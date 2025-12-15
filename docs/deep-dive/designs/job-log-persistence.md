# Job Execution Log Persistence

## Goals

- Persist job-scoped logs so job authors and operators can inspect executions alongside job/trigger metadata.
- Keep persistence optional and non-blocking for the hot path; default to memory-only when the feature is disabled.
- Preserve multi-tenant isolation (`TenantId`, `EnvironmentTag`) and let retention policies trim log data independently of job/trigger lifetime.

## Current State

- Jobs already receive an `ILogger` via `IJobExecutionContext`. The execution pipeline enriches scopes with `croniq.job.*`, `croniq.tenant_id`, `croniq.environment`, and optional trigger metadata, but nothing is persisted.
- EF Core persists jobs, triggers, and dead letters via `Croniq.Data.SqlServer`; there is no execution/log table yet. Correlation IDs exist only on webhook events (`WebhookEndpointEventEntity`); scheduler entities do not capture correlation today.
- Serilog/OpenTelemetry emit logs to external sinks, but there is no first-party query API for per-job execution logs.
- UX: Job authors should log via `context.Logger` so scopes (`JobKey`, `ExecutionId`, `CorrelationId`, tenant/env) stay intact. Samples should demonstrate structured logging with the provided logger rather than constructing new loggers.

## Proposed Design

### Data Model (EF Core)

- Introduce `JobExecutionRecord` to anchor each run:
  - `ExecutionId` (Guid, primary key), `JobKey`, `TenantId`, `EnvironmentTag`
  - Optional `TriggerId` (long) + `TriggerKey` (string) captured from the lease; store as nullable values without an FK to avoid cascading deletes when triggers are pruned.
  - Timing + outcome: `StartedAtUtc`, `CompletedAtUtc`, `Status` (`Succeeded`, `Failed`, `Canceled`), `DurationMs`, `ErrorType`, `ErrorMessage`, `PolicySnapshotJson`.
  - Traceability: `TraceId`, `SpanId`, `CorrelationId` (propagated from API/gRPC headers or metadata), `InstanceId`.
- Persist logs in one of three modes (configurable):
  1. **Structured rows (default)**: `JobExecutionLogEntry`
     - Columns: `Id` (identity), `ExecutionId`, `JobKey`, `TenantId`, `EnvironmentTag`, `TimestampUtc`, `Level`, `MessageTemplate`, `RenderedMessage`, `ExceptionJson`, `PropertiesJson`, `TraceId`, `SpanId`, `CorrelationId`, `Sequence`.
     - Index `IX_JobExecutionLogEntry_ExecutionId_Sequence` to stream ordered entries; optional filtered index on `(JobKey, Level)` for recent-error queries.
  2. **Chunked NDJSON**: `JobExecutionLogChunk`
     - Columns: `Id`, `ExecutionId`, `ChunkNumber`, `Payload` (`nvarchar(max)` NDJSON, optionally compressed), `FirstTimestampUtc`, `LastTimestampUtc`, `LineCount`, `CorrelationId`, `TraceId`.
     - Use fixed chunk sizes (e.g., 64-128 KB) to avoid oversized rows; append-only per execution.
  3. **Filesystem/Object storage** (preferred for SaaS volumes):
     - Store one file per execution as NDJSON; path pattern: `logs/{tenant}/{environment}/{jobKey}/{yyyy}/{MM}/{dd}/{executionId}.ndjson` (optionally `.ndjson.gz`).
     - A slim `JobExecutionRecord` row in SQL keeps `StorageKind=File|Object`, `StoragePath` (or URI), `ContentEncoding`, `ContentLength`, `ChunkCount`, `TraceId`, `CorrelationId`, `Status`, and timing/outcome fields. No per-line rows in SQL.
     - For multi-instance workers, write to shared storage (NFS in dev, S3/Azure Blob in SaaS) with tenant-prefixed folders; record a SHA256 hash in the record for integrity checks.
- No hard FK to `JobEntity`/`TriggerEntity`: retention jobs can delete log rows without blocking job deletions; `JobKey`/`TriggerKey` keep the link semantically intact.

### Write Path

- `TriggerWorker` assigns a new `ExecutionId` and stamps `TraceId`/`SpanId` + optional `CorrelationId` from incoming metadata (REST/gRPC header or trigger payload) before invoking the pipeline.
- Wrap the job scope in a `JobLogScope` containing `ExecutionId`, `JobKey`, `TriggerKey`, `TenantId`, `EnvironmentTag`, `TraceId`, `CorrelationId`, `InstanceId`.
- Add an opt-in `JobLogSinkProvider` (`ILoggerProvider`) that activates only when `Croniq:Logging:PersistJobLogs.Enabled == true`.
  - The provider buffers `LogEntry` DTOs on a bounded `Channel`.
  - A background writer flushes to `IJobLogStore.AppendAsync` in batches to avoid blocking job threads.
  - Failure modes: drop with metrics and emit a single warning per fault window; never block job completion on log persistence.
- `IJobLogStore` abstraction exposes `AppendAsync` (batch), `CompleteExecutionAsync` (to update `JobExecutionRecord` status/duration), and `TryCreateExecutionAsync` (insert-or-ignore on retries).
- EF implementation:
  - Structured mode: bulk-insert via `DbContext.AddRange` + `SaveChangesAsync` or `ExecuteSqlRaw` when batching large numbers.
  - NDJSON mode: concatenate log DTOs to NDJSON payloads and insert `JobExecutionLogChunk` rows. EF can handle `nvarchar(max)` payloads; for very large chunks use `DbCommand` with `SequentialAccess` to stream parameters and avoid loading into LOH.

### Read/Query Path

- `IJobLogReader` exposes:
  - `GetExecutionAsync(executionId)` (returns metadata/status).
  - `StreamEntriesAsync(executionId, levelFilter?, cancellationToken)` returning `IAsyncEnumerable<JobLogEntry>` ordered by `Sequence`.
  - `StreamNdjsonAsync(executionId, cancellationToken)` returning `IAsyncEnumerable<string>` of NDJSON chunks when chunked mode is active.
  - `OpenStreamAsync(executionId)` returning a stream over the stored file/blob when `StorageKind=File|Object`.
  - `FindRecentErrorsAsync(jobKey, since)` for UI/alerting.
- EF streaming options:
  - Structured mode can rely on `AsAsyncEnumerable` over ordered queries (EF Core streams results as they are read).
  - NDJSON mode can use `DbContext.Database.GetDbConnection()` + `DbCommand` with `CommandBehavior.SequentialAccess` to stream large `nvarchar(max)` payloads without buffering.
  - Filesystem/Object mode streams the NDJSON file directly; paging happens client-side (line-by-line).

### Configuration & Retention

- New options (example): `Croniq:Logging:PersistJobLogs:{Enabled,Mode=Structured|Ndjson|Filesystem,Level,MaxSizePerExecutionKb,ChunkSizeKb,RetentionDays,MaxEntriesPerExecution,BasePath|ObjectBucket,UseGzip}`.
- Retention job deletes expired `JobExecutionLogEntry`/`JobExecutionLogChunk` rows and prunes stale `JobExecutionRecord` entries; keep defaults modest (e.g., 7-30 days).
- Filesystem/Object retention: delete expired files/objects and clear `JobExecutionRecord` pointers; prefer bucket lifecycle rules in SaaS.
- Allow per-job overrides (e.g., only persist WARN+ for high-volume jobs).

### Correlation

- `ExecutionId` becomes the primary correlation key inside Croniq; every log and the execution record carry it.
- Propagate `CorrelationId` from API/gRPC headers (reuse `X-Croniq-CorrelationId`) into the trigger payload so workers can persist it.
- Persist `TraceId`/`SpanId` from `Activity.Current` to align with existing OTel traces.
- Note: today only `WebhookEndpointEventEntity` stores `CorrelationId`; the new execution/log tables should add correlation to the scheduler surface.

## Weiter gedacht

- Export helpers: download NDJSON for a single execution (support bundles) or push to external sinks (S3/Azure Blob) for long-term storage.
- UI/CLI: per-execution log viewer with filters (level, text search, timeframe) and a jump-to-error shortcut; overlay execution metadata (duration, policy outcome).
- Alert hooks: emit domain events when an execution records ERROR/CRITICAL so operators can wire webhooks/emails without parsing central logs.
- PII hygiene: allow a redaction filter that strips configured property keys before persistence; document safe defaults.
- Multi-store support: file-based log store for air-gapped/offline scenarios, plus an interface for streaming logs to vendor APIs if SQL storage is disabled.
- Backfill: one-time migration to copy recent Serilog sinks (if using file/Loki) into the new tables for continuity.
- Operators: API exposes `GET /tenants/{tenantId}/executions/{executionId}/logs` (NDJSON). Sample script `scripts/get-execution-logs.ps1` calls the endpoint (`-TenantId`, `-ExecutionId`, `-Endpoint`, `-ApiKey`); file store retention via `Croniq:Logging:Execution:Retention`.

## Suggested Implementation Stages

1. Abstractions + pipeline wiring: add `ExecutionId`/correlation propagation, `JobLogScope`, `IJobLogStore`/`IJobLogReader` contracts, and a no-op provider.
2. EF Core store + schema: add entities/migrations for `JobExecutionRecord` + `JobExecutionLogEntry` + optional `JobExecutionLogChunk`, implement structured mode, and wire options.
3. NDJSON mode + streaming reader and filesystem/object mode: add chunk writer/reader with sequential streaming; add file/blob writer with hashed artifacts and reader that streams NDJSON; surface CLI/API endpoints for fetching logs by execution/job.
4. Retention + tests: retention job, failure injection tests (drop-on-fault), and integration tests that assert persisted logs match emitted scopes/levels.
