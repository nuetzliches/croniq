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

- Implement `IExecutionPolicyPipelineProvider` that turns resolved `ExecutionPolicyOptions` into Polly v8 resilience pipelines. The provider composes strategies in deterministic Reihenfolge `Timeout` → `CircuitBreaker` → `Retry` (Dead-Letter folgt spaeter) und cached das Resultat je `JobKey` + Fingerprint der Optionen.
- Pipelines werden bei Options-Aenderungen automatisch erneuert (Fingerprint basiert auf allen Optionseigenschaften, inkl. Exception-Filter). Logging auf `ILogger<ExecutionPolicyPipelineProvider>` meldet ignorierte Exception-Typen oder Zeitueberschreitungen.

### Execution Pipeline

- `DefaultJobExecutionPipeline` injiziert jetzt `IPolicyResolver` + `IExecutionPolicyPipelineProvider`, resolved pro JobKey die aktuellen `ExecutionPolicyOptions` und fuehrt den Handler konsequent durch den Polly-Pipeline-Wrapper.
- Bei aktivem Timeout uebergibt der Pipeline-Token das Abbruchsignal an den Job (konfigurierbar via `CancelExecutionOnTimeout`), sonst bleibt das aufruferseitige Token massgeblich. Telemetrie-/Dead-Letter-Hooks folgen, sobald Persistenz und Metriken erweitert sind.

### Dead Letter Strategy

- Extend `IJobPersistenceProvider` with `MoveToDeadLetterAsync` (if not already). When retries exhausted or policy decides to DLQ, persist the payload, exception metadata, policy snapshot, and schedule automatic cleanup based on retention options.
- Provide In-Memory fallback for local dev.

### Telemetry Integration

- Metrics: `cronipolicy.retry_attempts`, `cronipolicy.deadletter_total`, `cronipolicy.circuit_open` counters. Implemented via `PolicyMetrics` in `Croniq.Core.Execution`, emitted by the resilience provider / dead-letter flow so operators see transitions without extra wiring.
- Logs: structured entries for each policy transition with `Policy` (`timeout`, `retry`, `circuit-breaker`, `dead-letter`), `JobKey`, `Attempt`/`Delay` (for retries), and `Reason` (exception type/message). `ExecutionPolicyPipelineProvider` and `TriggerWorker` already emit these warnings/information entries, so dashboards and alerts can consume them immediately.

## Backlog to Complete the Policy Engine Milestone

- [x] Define `ExecutionPolicyOptions` + override binding in `Croniq.Core` (`Options/Policies`).
- [x] Implement `PolicyOverrideOptions.Execution` hierarchy (mirroring Misfire/Quota) and extend `IPolicyResolver` to supply execution policies per job.
- [x] Add `ExecutionPolicyPipelineProvider` (Polly v8) with retry/timeout/circuit support, caching pipelines per job, und verdrahte `DefaultJobExecutionPipeline` damit.
- [x] Extend resilience pipeline with a Dead-Letter fallback once persistence contracts und SQL-Skripte bereitstehen (TriggerWorker now routes exhausted leases via `DeadLetterRequest`).
- [x] Emit policy outcome counters/metrics via the `ExecutionPolicyPipelineProvider` + `TriggerWorker` instrumentation (replaces earlier plan to wire it inside `DefaultJobExecutionPipeline`).
- [x] Extend persistence contracts for dead-letter writes/reads and update SqlServer EF migrations accordingly.
- [x] Provide integration tests in `Croniq.Core.Tests` + contract tests for persistence to validate dead-letter storage.
- [ ] Document policy configuration knobs in `docs/consumer/policies.md` and add examples to samples.
- [ ] Wire dashboards/alerts from the observability plan to include policy counters (ensure exporters emit them).

Deliverables include code, tests, docs, and dashboard updates. When the backlog is complete, mark the checklist item as done.
