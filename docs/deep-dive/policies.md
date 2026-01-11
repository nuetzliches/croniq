# Croniq Policy Engine Plan

This document explains how Croniq implements the Polly-based policy engine outlined in `architecture.md`. The goal is to close the "Polly-based policy engine" item in `CHECKLIST.md` by defining retry/timeout/circuit and dead-letter behavior across Scheduler and API workloads.

## Goals

- Provide deterministic policy resolution per job key (global -> tenant -> environment -> namespace -> job) leveraging the existing `IPolicyResolver` in `Croniq.Core`.
- Use Polly resilience pipelines to compose retry, timeout, circuit breaker, and fallback behaviors for every job execution.
- Ensure dead-letter routing and telemetry signals fire consistently regardless of policy override layer.
- Allow sample hosts to switch between default in-memory guards and future distributed implementations without code changes.

## Components

### Policy Options

- Introduce `ExecutionPolicyOptions` with sections:
  - `Retry`: enabled flag, max attempts, backoff strategy (fixed, linear, exponential), jitter, retryable exception filters.
  - `Timeout`: per execution timeout, cancellation propagation toggle.
  - `CircuitBreaker`: failure threshold, sampling window, cooldown, minimum throughput, break-on-exception predicate.
  - `DeadLetter`: toggle, retention days, manual intervention hints.
- Options integrate with `PolicyOverrideOptions` for hierarchical overrides (global defaults + per tenant/job). Config lives under `Croniq:Policies:*` with `Croniq.Api` binding via `IOptions`.

### Builder

- Implement `IExecutionPolicyPipelineProvider` that turns resolved `ExecutionPolicyOptions` into Polly v8 resilience pipelines. The provider composes strategies in deterministic order (`Timeout` -> `CircuitBreaker` -> `Retry`; dead-letter follows once wired) and caches the result per `JobKey` + options fingerprint.
- Pipelines are regenerated when option fingerprints change (fingerprint covers all option properties, including exception filters). `ILogger<ExecutionPolicyPipelineProvider>` logs ignored exception types or timeouts.

### Execution Pipeline

- `DefaultJobExecutionPipeline` now injects `IPolicyResolver` + `IExecutionPolicyPipelineProvider`, resolves `ExecutionPolicyOptions` per JobKey, and executes the handler through the Polly pipeline wrapper.
- When timeouts are enabled, the pipeline token forwards cancellation to the job (configurable via `CancelExecutionOnTimeout`); otherwise the caller token remains authoritative. Telemetry/dead-letter hooks follow once persistence and metrics are extended.

### Dead Letter Strategy

- Extend `IJobPersistenceProvider` with `MoveToDeadLetterAsync` (if not already). When retries exhausted or policy decides to DLQ, persist the payload, exception metadata, policy snapshot, and schedule automatic cleanup based on retention options.
- Provide In-Memory fallback for local dev.

### Telemetry Integration

- Metrics: `cronipolicy_retry_attempts`, `cronipolicy_deadletter_total`, `cronipolicy_circuit_open` counters. Implemented via `PolicyMetrics` in `Croniq.Core.Execution`, emitted by the resilience provider / dead-letter flow so operators see transitions without extra wiring.
- Logs: structured entries for each policy transition with `Policy` (`timeout`, `retry`, `circuit-breaker`, `dead-letter`), `JobKey`, `Attempt`/`Delay` (for retries), and `Reason` (exception type/message). `ExecutionPolicyPipelineProvider` and `TriggerWorker` already emit these warnings/information entries, so dashboards and alerts can consume them immediately.

## Configuration & Overrides

Croniq binds policy options via `IOptions<T>` so hosts can drive behavior from `appsettings.*` or environment variables.

- **Default sections**: `Croniq:Policies:Misfire` -> `MisfirePolicyOptions`, `Croniq:Policies:Execution` -> `ExecutionPolicyOptions`, `Croniq:Policies:Overrides` -> `PolicyOverrideOptions`.
- **Override resolution**: `PolicyOverrideOptions.Execution`/`Misfire` picks the most specific match (tenant > environment > namespace > job). `PolicyOverrideOptions.Quotas` applies every matching entry and chooses the most restrictive values (minimums).
- **Units & meaning**:
  - `Retry.MaxAttempts` counts the initial try; `InitialDelay`/`MaxDelay` are `TimeSpan` strings (`00:00:02`).
  - `Retry.RetryableExceptions` expects fully qualified type names; empty list means "retry everything except cancellations".
  - `CircuitBreaker.FailureThreshold` is a percentage (5 means 5%); `MinimumThroughput` is the minimum samples before evaluation.
  - `Timeout.Timeout` is a `TimeSpan`; `CancelExecutionOnTimeout` controls cooperative cancellation of the job handler.
  - `DeadLetter.Retention` is a `TimeSpan`; `OperatorHint` is surfaced on dead-letter entries.

### AppSettings example

```json
{
  "Croniq": {
    "Policies": {
      "Misfire": {
        "MaxMisfireDelay": "00:05:00",
        "DeadLetterOnMisfire": true,
        "RescheduleBackoff": "00:00:30"
      },
      "Execution": {
        "Retry": {
          "Enabled": true,
          "MaxAttempts": 4,
          "BackoffStrategy": "Exponential",
          "InitialDelay": "00:00:02",
          "MaxDelay": "00:00:30",
          "JitterFactor": 0.25,
          "RetryableExceptions": [
            "System.TimeoutException",
            "System.IO.IOException"
          ]
        },
        "Timeout": {
          "Enabled": true,
          "Timeout": "00:02:00",
          "CancelExecutionOnTimeout": true
        },
        "CircuitBreaker": {
          "Enabled": true,
          "FailureThreshold": 10,
          "SamplingWindow": "00:01:00",
          "BreakDuration": "00:00:30",
          "MinimumThroughput": 20
        },
        "DeadLetter": {
          "Enabled": true,
          "Retention": "30.00:00:00",
          "OperatorHint": "check downstream API"
        }
      },
      "Overrides": {
        "Execution": [
          {
            "TenantId": "default",
            "NamespaceSegment": "payments",
            "Options": {
              "Retry": {
                "MaxAttempts": 2,
                "RetryableExceptions": ["System.InvalidOperationException"]
              },
              "CircuitBreaker": {
                "Enabled": true,
                "FailureThreshold": 25,
                "SamplingWindow": "00:00:30",
                "BreakDuration": "00:00:20"
              }
            }
          },
          {
            "TenantId": "default",
            "EnvironmentTag": "prod",
            "JobName": "invoice",
            "Options": {
              "Timeout": { "Timeout": "00:00:30" },
              "DeadLetter": {
                "OperatorHint": "review invoice payload before replay"
              }
            }
          }
        ],
        "Quotas": [
          {
            "TenantId": "default",
            "NamespaceSegment": "payments",
            "Options": {
              "MaxTriggersPerMinute": 30,
              "MaxParallelExecutionsPerJob": 2
            }
          }
        ],
        "Misfire": [
          {
            "TenantId": "default",
            "NamespaceSegment": "billing",
            "Options": {
              "MaxMisfireDelay": "00:02:00",
              "DeadLetterOnMisfire": false,
              "RescheduleBackoff": "00:00:10"
            }
          }
        ]
      }
    }
  }
}
```

### Environment variables

The same configuration can be expressed for containers:

```bash
CRONIQ__POLICIES__EXECUTION__TIMEOUT__TIMEOUT=00:02:00
CRONIQ__POLICIES__EXECUTION__RETRY__MAXATTEMPTS=4
CRONIQ__POLICIES__OVERRIDES__EXECUTION__0__TENANTID=1
CRONIQ__POLICIES__OVERRIDES__EXECUTION__0__NAMESPACESEGMENT=payments
CRONIQ__POLICIES__OVERRIDES__EXECUTION__0__OPTIONS__CIRCUITBREAKER__FAILURETHRESHOLD=25
CRONIQ__POLICIES__OVERRIDES__QUOTAS__0__OPTIONS__MAXTRIGGERSPERMINUTE=30
```

### Host wiring

Add the bindings once per host so defaults and overrides flow into the `IPolicyResolver`:

```csharp
services.Configure<MisfirePolicyOptions>(configuration.GetSection("Croniq:Policies:Misfire"));
services.Configure<ExecutionPolicyOptions>(configuration.GetSection("Croniq:Policies:Execution"));
services.Configure<PolicyOverrideOptions>(configuration.GetSection("Croniq:Policies:Overrides"));
```

`AddCroniqObservability` already registers the `Croniq.Core.Policy` meter; keep it in your OpenTelemetry configuration when customizing instrumentation:

```csharp
builder.WithMetrics(metrics => metrics.AddMeter("Croniq.Core.Policy", "Croniq.Core"));
```

## Dashboards & Alerts

- **Metrics**: `cronipolicy_retry_attempts`, `cronipolicy_circuit_open`, `cronipolicy_deadletter_total` are emitted from the execution pipeline. Ensure your OTel collector forwards them to Prometheus (devstack does this by default).
- **Grafana**: `infra/docker/observability/grafana/dashboards/api-gateway.json` and `scheduler.json` surface the policy counters (retry/circuit/dead-letter) per tenant/environment. Load them via the devstack overlay or by mounting the JSON into your Grafana deployment.
- **Alerts**: `infra/monitoring/rules/scheduler-alerts.yaml` ships alert rules that watch `cronipolicy_deadletter_total` and related scheduler signals. Mount the rule file into Prometheus (already done in the devstack compose) to trigger `CroniqDeadLettersHigh` when DLQs rise.
- **Runbook hints**: Dead-letter entries store the `OperatorHint` alongside policy snapshots; include actionable text there to shorten triage when alerts fire.

## Backlog to Complete the Policy Engine Milestone

- [x] Define `ExecutionPolicyOptions` + override binding in `Croniq.Core` (`Options/Policies`).
- [x] Implement `PolicyOverrideOptions.Execution` hierarchy (mirroring Misfire/Quota) and extend `IPolicyResolver` to supply execution policies per job.
- [x] Add `ExecutionPolicyPipelineProvider` (Polly v8) with retry/timeout/circuit support, caching pipelines per job, and wire `DefaultJobExecutionPipeline` to use it.
- [x] Extend resilience pipeline with a Dead-Letter fallback once persistence contracts and SQL scripts are ready (TriggerWorker now routes exhausted leases via `DeadLetterRequest`).
- [x] Emit policy outcome counters/metrics via the `ExecutionPolicyPipelineProvider` + `TriggerWorker` instrumentation (replaces earlier plan to wire it inside `DefaultJobExecutionPipeline`).
- [x] Extend persistence contracts for dead-letter writes/reads and update SqlServer EF migrations accordingly.
- [x] Provide integration tests in `Croniq.Core.Tests` + contract tests for persistence to validate dead-letter storage.
- [x] Document policy configuration knobs in `docs/policies.md` and add examples to samples.
- [x] Wire dashboards/alerts from the observability plan to include policy counters (ensure exporters emit them).

Deliverables include code, tests, docs, and dashboard updates. When the backlog is complete, mark the checklist item as done.
