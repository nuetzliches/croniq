# Croniq Testing Strategy

This document expands on the quality strategy outlined in `CONCEPT.md` (section 12) and explains how we validate Croniq end-to-end.

## Goals

- Catch regressions quickly with fast unit tests while keeping provider and integration behavior validated.
- Provide deterministic, reproducible environments so failures are actionable both locally and in CI.
- Enforce coverage and quality gates (unit + contract on every PR, E2E + compliance checks nightly or before release).
- Make it trivial for contributors to add new tests by exposing shared fixtures via `Croniq.TestKit`.

## Test Levels

### 1. Unit Tests (per library)

- **Scope**: Pure logic in `src/*` projects (scheduling, policies, job metadata, hosting extensions).
- **Frameworks**: `xUnit` + `FluentAssertions` (to be added) with optional `NSubstitute` for lightweight fakes; stick to in-memory doubles, no network or file IO.
- **Structure**: Mirror namespaces (e.g., `Croniq.Core.Tests/Scheduling/TriggerWorkerTests.cs`). Keep Arrange/Act/Assert explicit, prefer data-driven `[Theory]` for parser/policy matrices.
- **Execution**: `dotnet test src/<Project>.csproj` or `dotnet test tests/Croniq.Core.Tests/Croniq.Core.Tests.csproj` with default configuration.
- **Gates**: PRs must pass all unit suites plus enforce `Coverlet` line/branch coverage ≥80% for `Croniq.Core`, ≥70% overall. Coverage reports are generated via `dotnet test /p:CollectCoverage=true /p:CoverletOutputFormat=cobertura` and uploaded to CI artifacts.

### 2. Contract Tests (provider boundaries)

- **Scope**: Interactions with external dependencies (Xtraq SQL procedures, Auth stores, provider abstractions such as `ISecretProvider`).
- **Frameworks**: `xUnit` + `Testcontainers for .NET` to spin up SQL Server 2022 or other dependencies per suite. `Croniq.TestKit` supplies:
  - Docker container lifecycle helpers (idempotent start/stop, log capture).
  - Seeder utilities for `infra/sql/xtraq` scripts (apply via `sqlcmd` on startup).
  - Strongly typed fixtures for ApiKey/Tenant seeds and default policy sets.
- **Structure**: Dedicated projects under `tests/*/*.ContractTests.cs`. For example, `Croniq.Persistence.Xtraq.Tests` contains `JobPersistenceContracts.cs` verifying CRUD semantics by calling the generated Xtraq client; `Croniq.Auth` contracts will be added once the provider exists.
- **Execution**: `dotnet test tests/Croniq.Persistence.Xtraq.Tests/Croniq.Persistence.Xtraq.Tests.csproj --filter Category=Contract` (categories applied via `[Trait("Category", "Contract")]`).
- **Gates**: Required on every PR (parallelizable in CI). Failures must expose container logs through `Croniq.TestKit` for diagnosis. Nightly runs also execute expensive permutations (e.g., failover scenarios, concurrency stress).

### 3. End-to-End & Smoke Tests

- **Scope**: Full Croniq stack—Scheduler worker + API + SQL + supporting services—running via Docker Compose under `infra/docker`. Validate real user flows: registering jobs, scheduling triggers, misfire recovery, Auth + RateLimiter enforcement.
- **Frameworks**: `Playwright` or `REST-assured`-style HTTP clients in `tests/Croniq.Api.Smoke` (to be created). Compose definition `infra/docker/docker-compose.tests.yml` boots the stack; tests run against `http://localhost:5080` (API) and gRPC endpoint `https://localhost:5081`.
- **Execution**: `docker compose -f infra/docker/docker-compose.tests.yml up --build -d`, wait for health checks, then `dotnet test tests/Croniq.Api.Smoke/Croniq.Api.Smoke.csproj`. Tear down via `docker compose ... down -v`.
- **Cadence**: Nightly + release candidates. Optional manual trigger before large refactors. Failures block release until resolved.

## Tooling & Infrastructure

- **Croniq.TestKit** (new project under `tests/`): shared helpers for DI bootstrapping, seeded tenants/api keys, deterministic clocks, container orchestration, and response snapshot utilities.
- **Static analysis**: Enable nullable reference types everywhere (already on) + .NET analyzers set to `warning` in test projects to catch flaky patterns.
- **Data management**: Database snapshots created via `infra/sql/xtraq/apply.ps1` for local dev; contract tests must tear down schema per run to avoid cross-test bleed.
- **Diagnostics**: Use `ITestOutputHelper` + structured logging to emit context (TenantId, ScheduleId). Contract/E2E suites push logs and traces to the Compose OTel Collector for triage.

## CI Pipelines

1. **PR pipeline** (GitHub Actions):
   - `dotnet format --verify-no-changes` (optional once formatting is enforced).
   - `dotnet test` for all unit + contract projects with `/p:CollectCoverage=true`.
   - Upload coverage + test results; enforce coverage gates.
2. **Nightly pipeline**:
   - Everything from PR pipeline.
   - Build Docker images, run Compose E2E suite, collect artifacts (logs, traces, Compose events).
   - Run security scans (Trivy) + SBOM generation (Syft) for awareness, though enforcement remains in release pipeline.
3. **Release pipeline**:
   - Full suite (unit, contract, E2E) on tagged commits.
   - Smoke deployment against staging environment (Kubernetes or Compose) with rollback plan per `CONCEPT.md` section 17.

## Developer Workflow

- Use `dotnet test` locally with `--filter Category=Unit` or `=Contract` to target suites.
- For contract tests, install Docker Desktop and run `./infra/sql/xtraq/apply.ps1` once to prime images; thereafter rely on Testcontainers automation.
- For E2E, reuse the same Compose files as CI; provide a helper script `./scripts/test-e2e.cmd` (future work) to orchestrate up/down flows and log aggregation.
- Document flaky scenarios immediately in `tests/README.md` (to be added) and open tracking issues.

## Backlog for the Testing Stream

- [ ] Create `Croniq.TestKit` with SQL Server Testcontainer fixture + seeded data builders.
- [ ] Add FluentAssertions/NSubstitute across test projects and refactor existing tests for readability.
- [ ] Introduce `[Category]` traits and update `Directory.Build.props` to enforce Coverlet instrumentation.
- [ ] Create `Croniq.Api.Smoke` project + Compose file for automated end-to-end runs.
- [ ] Publish developer guide (`docs/technical/testing.md` + `tests/README.md`) describing local setup, troubleshooting, and log collection.
- [ ] Wire GitHub Actions workflows (`.github/workflows/tests.yml`, `nightly.yml`) to run the described stages.

By following this plan we can move the “Teststrategie” item in `CHECKLIST.md` from open to done once the outlined backlog is completed.
