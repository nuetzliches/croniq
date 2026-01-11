# Croniq Deep-Dive Documentation

This section targets maintainers, platform engineers, and contributors working on Croniq itself. Consumer docs (Quickstart, configuration basics, etc.) live one directory higher. Everything under `/deep-dive` may assume familiarity with the product and focuses on operating, extending, or evolving the platform. For structure/ownership details, start with [`docstreams.md`](./docstreams.md).

## Contents (Planned)

- Architecture deep dive (`architecture.md`)
- Persistence model & SqlServer schema references
- Provider extension guides (logging, telemetry, secrets, etc.)
- Deployment playbooks (Docker Compose, Kubernetes, CI/CD)
- Observability standards (Serilog + OpenTelemetry)
- Release, compliance, and security checklists
- Testing strategy & tooling (`testing.md`)
- Security baseline & auth flows (`security.md`)
- Observability plan (`observability.md`)
- Policy engine rollout (`policies.md`)
- CI/CD pipelines (`ci.md`)
- Docker dev stack (`devstack.md`) — canonical source for Compose profiles, debugging shortcuts, and advanced setup steps referenced from the consumer Quickstart.
- Documentation streams (`docstreams.md`)
- Supply chain & release security (`supplychain.md`)
- UI backlog & strategy (`ui.md`)
- Kubernetes chart plan (`kubernetes.md`)

## Provider Contracts (Draft)

- Logging: `ILoggingProvider` supplies loggers per category/type; default implementation can wrap `ILoggerFactory`.
- Telemetry: `ITelemetryProvider` surfaces `ActivitySource` and `Meter` creation to align tracing/metrics across components.
- Secrets: `ISecretProvider` resolves `SecretRequest` (name/version/scope) to `SecretValue` with optional expiry metadata.

### Default Provider Implementations

- `Croniq.Providers.Default` registers:
  - Logging via `LoggerFactoryProvider` (bridges `ILoggerFactory`).
  - Telemetry via `DefaultTelemetryProvider` (caches `ActivitySource`/`Meter` instances).
  - Secrets via `InMemorySecretProvider` for dev/test; add with `AddCroniqInMemorySecrets(opts => opts.Secrets["api-key"] = "...")`.

### Service Layer Skeleton

- `Croniq.Api` provides Minimal API endpoints:
  - `GET /health`
  - `POST /tenants/{tenantId}/schedules` (job + trigger upsert via `IJobPersistenceProvider`)
  - `GET/DELETE /tenants/{tenantId}/schedules/{id}`
  - `POST /jobs/trigger` (direct invocation via `IJobExecutionPipeline`)
  - Fixed-window rate limiting and simple API-key guard (`X-Croniq-Key`, see `CroniqApiOptions`).
- `Croniq.Rpc.Client` packages generated gRPC clients (`Protos/scheduler.proto`, `worker.proto`, `webhook_ingress.proto`) plus helpers for Scheduler/Worker/Webhook ingress operations.

## Authoring Guidelines

- Keep explanations in English and reference specific sections of `architecture.md` whenever context helps.
- Provide diagrams or sequence sketches when describing execution flows, clustering, or recovery logic.
- Cross-link consumer-facing docs when relevant (e.g., "see `../quickstart.md` for the client perspective").
