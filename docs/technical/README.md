# Croniq Technical Documentation

This section is intended for maintainers, platform engineers, and contributors working on Croniq itself.

## Contents (Planned)

- Architecture deep dive (extends `CONCEPT.md`)
- Persistence model & Xtraq schema references
- Provider extension guides (logging, telemetry, secrets, etc.)
- Deployment playbooks (Docker Compose, Kubernetes, CI/CD)
- Observability standards (Serilog + OpenTelemetry)
- Release, compliance, and security checklists
- Testing strategy & tooling (`testing.md`)
- Security baseline & auth flows (`security.md`)

## Provider contracts (draft)

- Logging: `ILoggingProvider` supplies loggers per category/type; default implementation can wrap `ILoggerFactory`.
- Telemetry: `ITelemetryProvider` surfaces `ActivitySource` and `Meter` creation to align tracing/metrics across components.
- Secrets: `ISecretProvider` resolves `SecretRequest` (name/version/scope) to `SecretValue` with optional expiry metadata.

### Default provider implementations

- `Croniq.Providers.Default` registers:
  - Logging via `LoggerFactoryProvider` (bridges `ILoggerFactory`).
  - Telemetry via `DefaultTelemetryProvider` (caches `ActivitySource`/`Meter` instances).
  - Secrets via `InMemorySecretProvider` for dev/test; add with `AddCroniqInMemorySecrets(opts => opts.Secrets["api-key"] = "...")`.

### Service layer skeleton

- `Croniq.Api` provides Minimal API endpoints:
  - `GET /health`
  - `POST /schedules` (job + trigger upsert via `IJobPersistenceProvider`)
  - `POST /jobs/trigger` (direct invocation via `IJobExecutionPipeline`)
  - Fixed-window rate limiting and simple API-key guard (`X-Croniq-Key`, see `CroniqApiOptions`).
- `Croniq.Rpc.Client` contains the draft gRPC proto (`Protos/scheduler.proto`) for Scheduler operations (health, trigger, upsert/delete schedule); codegen to be added later.

## Authoring Guidelines

- Keep explanations in English and reference specific sections of `CONCEPT.md` whenever possible.
- Provide diagrams or sequence sketches when describing execution flows, clustering, or recovery logic.
- Cross-link consumer-facing docs when relevant (e.g., "see `../consumer/quickstart.md` for the client perspective").
