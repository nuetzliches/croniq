# Croniq Architecture

This document captures the current architecture of Croniq and replaces the earlier concept drafts. It records the validated design choices, service layout, and quality targets that already shape the codebase.

## Product Goals & SLOs

- Provide a modular .NET 10 scheduling platform with a light in-memory execution path, extensible providers, and durable persistence when required.
- Match the feature coverage of established schedulers (Quartz.NET) while lowering boot time and simplifying auth/policy setup.
- Default SLOs:
  - Trigger lookup + schedule evaluation: < 100 ms p50 / < 250 ms p95 for up to 10k active triggers per node.
  - End-to-end job start (trigger to `ExecuteAsync`): < 500 ms p95 (in-memory), < 750 ms p95 (SqlServer persistence).
  - Availability: 99.9% monthly for Scheduler/API; API error rate < 0.1% per day; clock drift between nodes < 50 ms.

## Layered Architecture

| Layer                                                                          | Responsibilities                                                                             |
| ------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------- |
| Scheduler Core (`Croniq.Core`)                                                 | Trigger parsing, schedule evaluation, policies, execution pipeline, `IJob` contracts.        |
| Provider Layer (`Croniq.Persistence.*`, `Croniq.Auth.*`, `Croniq.Providers.*`) | Abstractions and default implementations for persistence, auth, logging, telemetry, secrets. |
| Service Layer (`Croniq.Api`, RPC)                                              | Minimal API endpoints, rate limiting, auth middleware, gRPC/JSON-RPC endpoints.              |
| Jobs Layer (`Croniq.Sdk`, sample job projects)                                 | Authoring model for jobs, DI helpers, samples.                                               |
| Infrastructure (`infra/docker`, `tools/Croniq.DbMigrator`)                     | Docker Compose dev stack, EF Core migrations, helper scripts, future UI.                     |

Hosting extensions (`AddCroniqApiServices`, `AddCroniqApiRateLimiter`, `UseCroniqApi`) wire the pieces together. Auth and persistence can run in `InMemory` or `SqlServer` modes via configuration (`Croniq:*`).

## Repository & Documentation Layout

```
src/
  Croniq.Core/
  Croniq.JobStore.InMemory/
  Croniq.Persistence.Abstractions/
  Croniq.Persistence.SqlServer/
  Croniq.Auth.Abstractions/
  Croniq.Auth.Core/
  Croniq.Auth.SqlServer/
  Croniq.Data.SqlServer/
  Croniq.Api/
  Croniq.Rpc.Client/
  Croniq.Sdk/
  ...
docs/
  introduction/
  guides/
  ops/
  deep-dive/
```

- `Croniq.Data.SqlServer` centralises EF Core entities, DbContext, and migrations for both persistence and auth.
- `docs` is split into consumer-focused introductions/guides and the deep-dive stream (this document, security, observability, etc.).
- Architecture diagrams live in `docs/architecture.drawio`; edit via VS Code (hediet.drawio extension) or diagrams.net.

## Persistence & Auth

- SqlServer is the canonical durable store. All entities live in schema `croniq` with `TenantId`, `EnvironmentTag`, and row metadata.
- `Croniq.Persistence.SqlServer` implements `IJobPersistenceProvider`, handling jobs, triggers, leases, and dead letters. The CLI `tools/Croniq.DbMigrator` applies migrations.
- Auth shares the same DbContext. `Croniq.Auth.SqlServer` stores API clients/keys (hashed secrets with per-key salt); `Croniq.Auth.Core` offers the in-memory fallback for samples/tests.
- Config:
  - `Croniq:Auth:Mode = InMemory|SqlServer`; overrides under `Croniq:Auth:SqlServer:*`.
  - `Croniq:Persistence:Mode = InMemory|SqlServer`; overrides under `Croniq:Persistence:SqlServer:*`.
  - Shared connection string at `Croniq:SqlServer:ConnectionString` unless overridden.
- API key flow (header `X-Croniq-Key`) is the default. OIDC/OAuth2 bearer flow feeds the same `ICallerContext` abstraction. Rate limiting partitions on `TenantId:CallerId`.

## Scheduler & Execution Semantics

- Trigger types: Quartz-compatible cron (7 fields), fixed/sliding intervals, calendar windows, ad-hoc/event driven.
- Every job key follows `TenantId:EnvironmentTag:Namespace:JobName[:Variant]`, ensuring partitioning across tenants/environments.
- `IJobExecutionContext` exposes metadata, logger, telemetry hooks, and helpers (`InitProgress`, `ReportProgress`, `CustomState`). Jobs log and rethrow exceptions so policies can respond.
- Misfires are retried while `MaxMisfireDelay` (default 5 minutes) is respected. Beyond that the execution is marked as dead letter.
- Delivery semantics: at-least-once by default. Callers may supply `IdempotencyKey` metadata to deduplicate downstream effects.

## Job Store & Provider Model

- In-memory JobStore (`Croniq.JobStore.InMemory`) stays the default for dev/test while all operations flow through `IJobPersistenceProvider` contracts.
- SqlServer provider adds durability, leases, recovery, and future clustering. Locking uses SQL row versions and optional lease tables.
- Providers also exist for auth, logging (Serilog), telemetry (OpenTelemetry), secrets, and future extensions (queues, notifications).
- All persistence access goes through EF Core. Schema changes use migrations (`dotnet ef migrations add ...` in `Croniq.Data.SqlServer`). CI verifies drift via `Croniq.DbMigrator --verify`.

## API & RPC Surface

- Minimal API endpoints: `POST /jobs/trigger`, `POST /schedules`, `GET/DELETE /schedules/{id}`, `GET /health` (anonymous).
- gRPC service (`SchedulerService`) offers strongly typed access (`TriggerJob`, `RegisterSchedule`, `GetSchedules`). JSON-RPC may appear later but is not core.
- Hosting packages expose opinionated extensions so consumers can bootstrap Croniq quickly. Auth, persistence mode, and rate limits are all configuration-driven.
- Rate limiting uses ASP.NET Core RateLimiter with per-tenant partitions; gRPC interceptors reuse the same policy.

## Job Authoring Model

- `Croniq.Sdk` defines `[CroniqJob]` attribute, `IJob`, DI helpers (`AddCroniqJob<T>` / `AddCroniqJob(key, builder => ...)`).
- Jobs live in dedicated class libraries per domain. Packaging guidelines recommend NuGet distribution to enforce versioning.
- Fluent handler patterns: basic `Handle`, batch handlers, stateful handlers, progress reporting, custom states.

## Policies & Error Handling

- Policy engine builds on Polly (retry, timeout, circuit, fallback). Policies attach via the fluent builder or configuration, applying order-defined behavior.
- Dead-letter routing persists payload and reason, default retention 30 days (configurable). Recoveries feed into admin tooling via SqlServer tables.
- Idempotency tokens derive from metadata when needed; concurrency controls limit overlapping runs per job.

## Observability & Deployment

- Logging: Serilog with OpenTelemetry sink; structured fields include tenant/environment/job context.
- Metrics/Traces: OpenTelemetry SDK emitting OTLP by default. Dev stack ships with collector + Grafana + Tempo + Prometheus + Loki (optional).
- Docker strategy: multi-stage .NET 10 images, Compose stack for API, worker, SQL Server, observability, optional RPC sample.
- GitHub Actions build/test/publish NuGet packages and OCI images, uploading docs previews as artifacts.

## Reliability, Recovery & Multi-Tenancy

- Recovery flow: on startup, load persisted triggers, clean stale locks, resume pending executions before declaring the instance healthy.
- Clock drift monitoring via `ITimeProvider`; warnings from 50 ms drift upward.
- Retention defaults: dead letters 30 days, execution history 90 days, audit logs 365 days (all configurable).
- Multi-tenancy is enforced everywhere: persistence schemas, caller context, telemetry dimensions, rate limiting, and API scopes.
- Quotas per tenant cover max trigger/minute, concurrent executions, and payload size; defaults live under `Croniq:Api:RequestsPerMinute` with overrides per tenant.

## Release, Testing & Compliance

- Versioning: SemVer for libraries/SDKs, `/v1` routes for HTTP APIs. Breaking changes require new majors plus deprecation windows.
- Testing strategy: unit tests (xUnit) + contract tests (provider fixtures/Testcontainers) + Compose-based smoke/e2e suites (nightly and pre-release). Coverage gates: ≥80% core, ≥70% overall.
- Release pipeline runs migrations (`Croniq.DbMigrator`), executes smoke tests, produces SBOMs (Syft) and signs artifacts (Cosign/SignPath). Dependency updates are scanned via Trivy/Snyk and tracked by Renovate.

## Roadmap Snapshots

- **UI**: Deferred until API/providers stabilize. Requirements include schedule overview, trigger management, execution history.
- **Kubernetes**: Future Helm chart (`charts/croniq`) with API/worker deployments, SQL Server StatefulSet, migration jobs, ExternalSecrets integration, and pre-wired observability.
- **Clustering**: Arrives with GA SqlServer provider once lease/heartbeat tables are production-ready. Cluster health endpoints (`/cluster/nodes`) will expose status.

---

Future architectural decisions should update this document (and referenced deep-dive sheets) directly.
