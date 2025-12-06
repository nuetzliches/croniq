# Croniq Policy Engine Plan

This document explains how Croniq will implement the Polly-based policy engine promised in `CONCEPT.md` sections 5, 11, and 14. The goal is to close the "Policy-Engine auf Polly-Basis" item in `CHECKLIST.md` by defining retry/timeout/circuit and dead-letter behavior across Scheduler and API workloads.

## Goals

- Provide deterministic policy resolution per job key (global → tenant → environment → namespace → job) leveraging the existing `IPolicyResolver` in `Croniq.Core`.
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

- Create `IPolicyPipelineBuilder` producing `ResiliencePipeline<JobExecutionContext>` instances (Polly v8). Builder composes handlers in deterministic order: `Timeout` → `CircuitBreaker` → `Retry` → `DeadLetterFallback`.
- Pipelines cached per `JobKey` to avoid rebuild overhead; invalidated when options change (tie into `IOptionsMonitor`).

### Execution Pipeline

- `DefaultJobExecutionPipeline` injects `IPolicyPipelineProvider` and wraps handler invocation: `await pipeline.ExecuteAsync(async token => await descriptor.Handler.ExecuteAsync(ctx, token), cancellationToken);`
- On failure, pipeline emits structured events (log + meter) including policy outcome (`retry`, `breaker-open`, `dead-letter`), storing context via `JobExecutionTelemetry` helper.

### Dead Letter Strategy

- Extend `IJobPersistenceProvider` with `MoveToDeadLetterAsync` (if not already). When retries exhausted or policy decides to DLQ, persist the payload, exception metadata, policy snapshot, and schedule automatic cleanup based on retention options.
- Provide In-Memory fallback for local dev.

### Telemetry Integration

- Metrics: `cronipolicy.retry_attempts`, `cronipolicy.deadletter_total`, `cronipolicy.circuit_open` counters.
- Logs: structured entries for each policy transition with `JobKey`, `Attempt`, `Policy`, `Reason`.

## Backlog to Complete the Policy Engine Milestone

- [ ] Define `ExecutionPolicyOptions` + override binding in `Croniq.Core` (`Options/Policies`).
- [ ] Implement `PolicyOverrideOptions.Execution` hierarchy (mirroring Misfire/Quota) and extend `IPolicyResolver` to supply execution policies per job.
- [ ] Add `PolicyPipelineBuilder` (Polly v8) with retry/timeout/circuit/dead-letter support, caching pipelines per job.
- [ ] Update `DefaultJobExecutionPipeline` to run handlers through the resilience pipeline and surface telemetry signals.
- [ ] Extend persistence contracts for dead-letter writes/reads and update Xtraq SQL scripts accordingly.
- [ ] Provide integration tests in `Croniq.Core.Tests` + contract tests for persistence to validate dead-letter storage.
- [ ] Document policy configuration knobs in `docs/consumer/policies.md` and add examples to samples.
- [ ] Wire dashboards/alerts from the observability plan to include policy counters (ensure exporters emit them).

Deliverables include code, tests, docs, and dashboard updates. When the backlog is complete, mark the checklist item as done.
