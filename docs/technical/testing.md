# Croniq Testing Strategy

This document expands on the quality strategy outlined in `CONCEPT.md` (section 12) and explains how we validate Croniq end-to-end.

## Goals

- Catch regressions quickly with fast unit tests while keeping provider and integration behavior validated.
- Provide deterministic, reproducible environments so failures are actionable both locally and in CI.
- Enforce coverage and quality gates (unit + contract on every PR, E2E + compliance checks nightly or before release).
- Make it trivial for contributors to add new tests by exposing shared fixtures via `Croniq.TestKit`.

## Test Matrix (living reference)

| Suite                                    | Primary scope                                                                                | Trigger/Cadence             | Tooling / Infra                                               | Blocking rule                     |
| ---------------------------------------- | -------------------------------------------------------------------------------------------- | --------------------------- | ------------------------------------------------------------- | --------------------------------- |
| `Unit` (`tests/Croniq.*.Tests`)          | Pure logic, options, schedulers, API surface guards                                          | Every PR + local pre-push   | `xUnit`, `FluentAssertions`, `dotnet test`                    | Fail block merge                  |
| `Contract` (`*.ContractTests`)           | Provider contracts (Xtraq SQL, Auth, Secrets) via Testcontainers                             | Every PR (parallel)         | `Testcontainers`, seeded SQL, `Croniq.TestKit`                | Fail block merge                  |
| `Smoke`/`E2E` (`tests/Croniq.Api.Smoke`) | Croniq API + Worker SampleHosts via Compose (InMemory auth, SQL/Xtraq persistence, migrator) | Nightly + release candidate | `scripts/test-e2e.cmd` (wraps Docker Compose + `dotnet test`) | Fail blocks release/nightly badge |
| `Compliance`                             | SBOM, Trivy scan, dependency audit                                                           | Nightly + release           | `Syft`, `Trivy`, GH Actions reusable workflows                | Fail blocks release               |
| `Perf/Burn-in` (future)                  | Long-running stress on scheduler leases + quotas                                             | On-demand / before GA       | Testcontainers + perf harness (to be defined)                 | Informational                     |

## Test Levels

### 1. Unit Tests (per library)

- **Scope**: Pure logic in `src/*` projects (scheduling, policies, job metadata, hosting extensions).
- **Frameworks**: `xUnit` + `FluentAssertions` (rolling adoption) with optional `NSubstitute` for lightweight fakes; stick to in-memory doubles, no network or file IO.
- **Structure**: Mirror namespaces (e.g., `Croniq.Core.Tests/Scheduling/TriggerWorkerTests.cs`). Keep Arrange/Act/Assert explicit, prefer data-driven `[Theory]` for parser/policy matrices.
- **Execution**: `dotnet test src/<Project>.csproj` or `dotnet test tests/Croniq.Core.Tests/Croniq.Core.Tests.csproj` with default configuration.
- **Gates**: PRs must pass all unit suites plus enforce `Coverlet` line/branch coverage ≥80% for `Croniq.Core`, ≥70% overall. Coverage reports are generated via `dotnet test /p:CollectCoverage=true /p:CoverletOutputFormat=cobertura` and uploaded to CI artifacts.

### 2. Contract Tests (provider boundaries)

- **Scope**: Interactions with external dependencies (Xtraq SQL procedures, Auth stores, provider abstractions such as `ISecretProvider`).
- **Frameworks**: `xUnit` + `Testcontainers for .NET`. `Croniq.TestKit` now bootstraps SQL Server 2022 automatically:
  - `XtraqDatabaseFixture` spins up an ephemeral SQL Server container via `DotNet.Testcontainers` whenever `CRONIQ_SQL` is not provided, or reuses the supplied connection string for pre-provisioned environments.
  - `SqlScriptBatchExecutor` applies every `infra/sql/xtraq` script (GO-aware) before each run and seeds the default tenant + instance so suites always start clean.
  - `CreateProvider()` wires `Croniq.Persistence.Xtraq` with logging, so contract tests can resolve `IJobPersistenceProvider` without custom DI boilerplate.
  - `TestCategories` provides canonical `[Trait]` keys/values (e.g., `Category=Contract`) so suites can be filtered consistently via `dotnet test --filter`.
  - `CaptureContainerLogsAsync` + `TestcontainerLogCollector` persist SQL Server container logs to disk when diagnosing failures (automate collection in CI later).
- **Structure**: Dedicated projects under `tests/*/*.ContractTests.cs`. For example, `Croniq.Persistence.Xtraq.Tests` contains `XtraqJobPersistenceProviderTests.cs` verifying CRUD semantics at the stored procedure boundary; `Croniq.Auth` contracts will be added once the provider exists.
- **Execution**: `dotnet test tests/Croniq.Persistence.Xtraq.Tests/Croniq.Persistence.Xtraq.Tests.csproj --filter Category=Contract` (categories applied via `[Trait("Category", "Contract")]`).
- **Gates**: Required on every PR (parallelizable in CI). Failures should include SQL container logs (collection hooks tracked in the TestKit backlog) and are investigated before merge. Nightly runs execute additional permutations (failover, concurrency stress).

### 3. End-to-End & Smoke Tests

- **Scope**: The Compose harness (`infra/docker/docker-compose.tests.yml`) now stands up SQL Server 2022, the `Croniq.DbMigrator` job, `Croniq.Api.SampleHost`, and `Croniq.Worker.SampleHost`. Auth still uses the in-memory provider for deterministic API keys, while persistence runs against the same Xtraq schema/lifetime that production uses. This ensures smoke runs validate health probes, schedule creation, and that the worker can lease and execute triggers end-to-end.
- **Frameworks**: `xUnit` + `FluentAssertions` HTTP harness located in `tests/Croniq.Api.Smoke`. Tests talk to the API over `HttpClient`, covering `/health` and `/schedules` flows. The worker host processes sample jobs from `Croniq.SampleJobs`, so trigger leases are exercised while tests run.
- **Execution**: Use `scripts\test-e2e.cmd` (requires Docker Desktop + .NET SDK). The script:
  1. Builds/starts the Compose stack, including SQL + migrator + API + worker.
  2. Polls `http://localhost:5080/health` (or the overridden `CRONIQ_API_BASEURL`) until healthy.
  3. Runs `dotnet test tests/Croniq.Api.Smoke/Croniq.Api.Smoke.csproj --nologo` with `CRONIQ_API_BASEURL`/`CRONIQ_API_KEY` defaults (`http://localhost:5080`, `smoke-key`).
  4. Tears the stack down with `docker compose ... down -v`, regardless of success/failure.
- **Cadence**: Manual before large API refactors and nightly/regression once CI automation lands. Failures block release readiness because they represent real entry-point regressions.

## Tooling & Infrastructure

- **Croniq.TestKit** (new project under `tests/`): shared helpers for DI bootstrapping plus the `XtraqDatabaseFixture`, SQL batch executor, repository path resolver, deterministic `TestClock`, payload builders for jobs/triggers, and default tenant/instance seeders (with Docker-backed SQL when needed). Future milestones will add response snapshot utilities.
- **Static analysis**: Enable nullable reference types everywhere (already on) + .NET analyzers set to `warning` in test projects to catch flaky patterns.
- **Data management**: Database snapshots created via `infra/sql/xtraq/apply.ps1` for local dev; contract tests must tear down schema per run to avoid cross-test bleed.
- **Diagnostics**: Use `ITestOutputHelper` + structured logging to emit context (TenantId, ScheduleId). Contract/E2E suites push logs and traces to the Compose OTel Collector for triage.

## CI Pipelines

1. **PR pipeline** (GitHub Actions):
   - Stage `lint`: `dotnet format --verify-no-changes` (add once formatter config finalized).
   - Stage `build`: `dotnet restore` (with `actions/cache` for NuGet) + `dotnet build` (Release) for all solutions.
   - Stage `unit-contract`: `dotnet test tests/Croniq.Core.Tests/Croniq.Core.Tests.csproj /p:CollectCoverage=true /p:CoverletOutputFormat="cobertura"` and repeat for every `tests/*` project tagged `Category!=E2E`.
   - Stage `artifacts`: publish `TestResults/*.trx`, `coverage/*.xml`, and docker logs emitted by contract suites.
   - Gates: enforce per-project coverage (≥80% `Croniq.Core`, ≥70% repo) using `reportgenerator` or `coverlet merge` + `coverlet report` script.
2. **Nightly pipeline**:
   - Reuse PR template (matrix build) + add `docker-build` (push to GHCR `nuetzliches/croniq-nightly` with `:sha` tag).

- Stage `e2e-compose`: invoke `scripts/test-e2e.cmd` (or replicate its steps) to build the stack, wait for `/health`, run the smoke project, and tear down. Collect Grafana/OTel traces and container logs as artifacts.
- Stage `security`: run `syft packages . -o json` (upload SBOM) + `trivy fs --exit-code 1 --severity HIGH,CRITICAL .`.

3. **Release pipeline**:
   - Triggered on tags `v*`.
   - Stage `full-test`: matrix of unit, contract, E2E (same as nightly but blocking).
   - Stage `package`: publish NuGet packages + container images with semver tags.
   - Stage `smoke-deploy`: deploy Helm/Compose to staging, run `tests/Croniq.Api.Smoke` against staging URL, collect logs, and provide rollback instructions referencing `CONCEPT.md` §17.

## Developer Workflow

- Use `dotnet test` locally with `--filter Category=Unit` or `=Contract` to target suites.
- For contract tests, ensure Docker Desktop (or another Docker runtime) is running. By default `XtraqDatabaseFixture` launches SQL Server 2022 in a container and reapplies `infra/sql/xtraq` for a clean slate each run. Set `CRONIQ_SQL` to reuse an existing database (the fixture will still ensure schema + seeds) and only run `infra/sql/xtraq/apply.ps1` manually when preparing that long-lived instance. Call `CaptureContainerLogsAsync()` after failures to persist SQL logs locally (CI automation follows).
- Use `TestClock` when policy or scheduling logic relies on deterministic timestamps and the builders in `Croniq.TestKit.Builders` to create jobs/triggers without repeating boilerplate.
- For smoke tests, run `scripts\test-e2e.cmd`. It builds the Compose stack, waits for `/health`, runs `dotnet test tests/Croniq.Api.Smoke/Croniq.Api.Smoke.csproj --nologo`, and tears everything down. Override `CRONIQ_API_BASEURL`/`CRONIQ_API_KEY` before invoking the script when targeting remote environments (defaults remain `http://localhost:5080` and `smoke-key`).
- Document flaky scenarios immediately in `tests/README.md` (to be added) and open tracking issues.

## Backlog for the Testing Stream

- Delivered: `tests/Croniq.TestKit/` project with repository locator, GO-aware SQL batch executor, and `XtraqDatabaseFixture` that spins up SQL Server 2022 (or reuses `CRONIQ_SQL`) and seeds the default tenant + instance.
- Owners: Core + Persistence maintainers.
- [x] Extend `Croniq.TestKit` utilities (deterministic clock, payload builders, container log capture).
  - Delivered: `TestClock`, `JobDefinitionBuilder`, `TriggerDefinitionBuilder`, and `TestcontainerLogCollector` + `XtraqDatabaseFixture.CaptureContainerLogsAsync` for exporting SQL Server logs.
  - Next: add response snapshot helpers + hook log export into CI artifacts.
- [x] Add FluentAssertions/NSubstitute across test projects and refactor existing tests for readability.
  - Deliverables: package references, shared assertions helpers, lint rule to forbid bare `Assert.True/False`.
  - Status: `Croniq.Persistence.Xtraq.Tests`, `Croniq.Core.Tests`, `Croniq.JobStore.InMemory.Tests`, and `Croniq.Providers.Default.Tests` migrated; extend to remaining suites.
- [x] Introduce `[Category]` traits and update `Directory.Build.props` to enforce Coverlet instrumentation.
  - Delivered: `TestCategories` helper + `[Trait]` annotations in contract suites and repository-level `Directory.Build.props` enabling automatic Coverlet output for every `*.Tests` project.
- [x] Create `Croniq.Api.Smoke` project + Compose file for automated end-to-end runs.
  - Delivered: `tests/Croniq.Api.Smoke/` HTTP harness exercising `/health` + `/schedules`, `infra/docker/docker-compose.tests.yml` wiring SQL + migrator + API + worker containers, `scripts/test-e2e.cmd` automation, and containerized sample hosts.
- [ ] Publish developer guide (`docs/technical/testing.md` + `tests/README.md`) describing local setup, troubleshooting, and log collection.
  - Deliverables: new `tests/README.md` with quickstart + troubleshooting tree; update this doc after each milestone.
- [ ] Wire GitHub Actions workflows (`.github/workflows/tests.yml`, `nightly.yml`) to run the described stages.
  - Deliverables: PR workflow covering lint/build/unit/contract, nightly workflow with compose E2E + scans, release workflow hooking into packaging + staging smoke tests.

By following this plan we can move the “Teststrategie” item in `CHECKLIST.md` from open to done once the outlined backlog is completed.
