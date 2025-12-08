# Croniq Testing Strategy

This document expands on the quality strategy outlined in `CONCEPT.md` (section 12) and explains how we validate Croniq end-to-end.

## Goals

- Catch regressions quickly with fast unit tests while keeping provider and integration behavior validated.
- Provide deterministic, reproducible environments so failures are actionable both locally and in CI.
- Enforce coverage and quality gates (unit + contract on every PR, E2E + compliance checks nightly or before release).
- Make it trivial for contributors to add new tests by exposing shared fixtures via `Croniq.TestKit`.

## Test Matrix (living reference)

| Suite                                                | Primary scope                                                                                                     | Trigger/Cadence             | Tooling / Infra                                               | Blocking rule                     |
| ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | --------------------------- | ------------------------------------------------------------- | --------------------------------- |
| `Unit` (`tests/Croniq.*.Tests`)                      | Pure logic, options, schedulers, API surface guards                                                               | Every PR + local pre-push   | `xUnit`, `FluentAssertions`, `dotnet test`                    | Fail block merge                  |
| `Contract` (`*.ContractTests`)                       | Provider contracts (SqlServer persistence/auth, secrets) via Testcontainers                                       | Every PR (parallel)         | `Testcontainers`, seeded SQL, `Croniq.TestKit`                | Fail block merge                  |
| `Observability` (`tests/Croniq.Observability.Tests`) | Verifies OTLP exporter wiring using an in-memory collector + host builder                                         | Nightly + ad-hoc PR runs    | `dotnet test`, ASP.NET minimal host, lightweight OTLP server  | Fail blocks nightly badge         |
| `Smoke`/`E2E` (`tests/Croniq.Api.Smoke`)             | `Croniq.Sample.ApiHost` + `Croniq.Sample.WorkerHost` via Compose (InMemory auth, SqlServer persistence, migrator) | Nightly + release candidate | `scripts/test-e2e.cmd` (wraps Docker Compose + `dotnet test`) | Fail blocks release/nightly badge |
| `Compliance`                                         | SBOM, Trivy scan, dependency audit                                                                                | Nightly + release           | `Syft`, `Trivy`, GH Actions reusable workflows                | Fail blocks release               |
| `Perf/Burn-in` (future)                              | Long-running stress on scheduler leases + quotas                                                                  | On-demand / before GA       | Testcontainers + perf harness (to be defined)                 | Informational                     |

## Test Levels

### 1. Unit Tests (per library)

- **Scope**: Pure logic in `src/*` projects (scheduling, policies, job metadata, hosting extensions).
- **Frameworks**: `xUnit` + `FluentAssertions` (rolling adoption) with optional `NSubstitute` for lightweight fakes; stick to in-memory doubles, no network or file IO.
- **Structure**: Mirror namespaces (e.g., `Croniq.Core.Tests/Scheduling/TriggerWorkerTests.cs`). Keep Arrange/Act/Assert explicit, prefer data-driven `[Theory]` for parser/policy matrices.
- **Execution**: `dotnet test src/<Project>.csproj` or `dotnet test tests/Croniq.Core.Tests/Croniq.Core.Tests.csproj` with default configuration.
- **Gates**: PRs must pass all unit suites plus the new `reportgenerator`-backed coverage gate (Croniq.Core line coverage ≥80%, repository line coverage ≥70%). Each `dotnet test` invocation emits Cobertura files into `coverage/<Suite>/coverage.cobertura.xml`, `reportgenerator` produces `coverage/report/Summary.json`, and the PR workflow fails immediately if either threshold is violated while still uploading the raw reports for analysis.

### 2. Contract Tests (provider boundaries)

- **Scope**: Interactions with external dependencies (SqlServer persistence/auth stores, provider abstractions such as `ISecretProvider`).
- **Frameworks**: `xUnit` + `Testcontainers for .NET`. `Croniq.TestKit` now bootstraps SQL Server 2022 automatically:
  - Die SQL-Container-Fixture startet bei Bedarf `mcr.microsoft.com/mssql/server:2022` via `DotNet.Testcontainers` oder nutzt `CRONIQ_SQL`, falls gesetzt.
  - Vor jedem Lauf wird `tools/Croniq.DbMigrator` ausgeführt, um EF-Core-Migrationen anzuwenden und Defaultdaten zu seeden, damit Suites deterministisch starten.
  - `CreateProvider()` wires `Croniq.Persistence.SqlServer` + `Croniq.Auth.SqlServer` mit Logging, sodass Contract-Tests `IJobPersistenceProvider` bzw. `IApiKeyStore` ohne Boilerplate auflösen.
  - `TestCategories` liefert kanonische `[Trait]`-Keys/Values (z.B. `Category=Contract`) für konsistente Filter (`dotnet test --filter`).
  - `CaptureContainerLogsAsync` + `TestcontainerLogCollector` persistieren SQL-Container-Logs für Troubleshooting und spätere CI-Artefakte.
- **Structure**: Dedizierte Projekte unter `tests/*/*.ContractTests.cs`. Beispiel: `Croniq.Persistence.SqlServer.Tests` enthält `SqlServerJobPersistenceProviderTests.cs`, die CRUD/Lease-Verhalten am EF-Core-Provider überprüfen; `Croniq.Auth`-Contracts folgen.
- **Execution**: `dotnet test tests/Croniq.Persistence.SqlServer.Tests/Croniq.Persistence.SqlServer.Tests.csproj --filter Category=Contract` (Traits siehe oben).
- **Gates**: Pflicht für jede PR (parallelisierbar). Fehler enthalten SQL-Container-Logs (Hooks liegen im TestKit-Backlog); nightly Läufe decken Failover + Concurrency ab.

### 3. Observability Smoke Tests

- **Scope**: Validates `CroniqObservabilityOptions`/`CroniqObservabilityExtensions` wiring by spinning up an in-memory OTLP HTTP collector, emitting one span/metric/log, and asserting that each signal reaches the collector.
- **Frameworks**: `xUnit`, ASP.NET minimal host (for the collector), and the OpenTelemetry SDK configured via `AddCroniqObservability`.
- **Execution**: `dotnet test tests/Croniq.Observability.Tests/Croniq.Observability.Tests.csproj`. No Docker dependencies are required; the test self-hosts the collector on a random loopback port.
- **Cadence**: Runs in the nightly GitHub Action before the compose stack spins up so exporter regressions fail fast; developers can run it locally whenever they touch observability configuration.
- **Gates**: Failure blocks the nightly badge/release readiness because it indicates broken telemetry export across all hosts.

### 4. End-to-End & Smoke Tests

- **Scope**: The Compose harness (`infra/docker/docker-compose.tests.yml`) now stands up SQL Server 2022, the `Croniq.DbMigrator` job, `Croniq.Sample.ApiHost`, and `Croniq.Sample.WorkerHost`. Auth bleibt InMemory für deterministische Keys, während Persistenz über denselben SqlServer-Provider/Migrationsstand läuft wie in Produktion. Dadurch validieren Smoke-Runs Health-Probes, Schedule-Erstellung und Trigger-Leases end-to-end.
- **Frameworks**: `xUnit` + `FluentAssertions` HTTP harness located in `tests/Croniq.Api.Smoke`. Tests talk to the API over `HttpClient`, covering `/health` and `/schedules` flows. The worker host processes sample jobs from `Croniq.Sample.Jobs`, so trigger leases are exercised while tests run.
- **Execution**: Use `scripts\test-e2e.cmd` (requires Docker Desktop + .NET SDK). The script:
  1. Builds/starts the Compose stack, including SQL + migrator + API + worker.
  2. Polls `http://localhost:5080/health` (or the overridden `CRONIQ_API_BASEURL`) until healthy.
  3. Runs `dotnet test tests/Croniq.Api.Smoke/Croniq.Api.Smoke.csproj --nologo` with `CRONIQ_API_BASEURL`/`CRONIQ_API_KEY` defaults (`http://localhost:5080`, `smoke-key`).
  4. Tears the stack down with `docker compose ... down -v`, regardless of success/failure.
- **Cadence**: Manual before large API refactors and nightly/regression once CI automation lands. Failures block release readiness because they represent real entry-point regressions.

## Tooling & Infrastructure

- **Croniq.TestKit** (new project under `tests/`): shared helpers for DI bootstrapping plus die SqlServer-Testcontainer-Fixture, ein Croniq.DbMigrator-Runner, Repository-Pfad-Resolver, deterministischen `TestClock`, Payload-Builder und Default-Seeds (bei Bedarf Docker-unterstützt). Zukünftige Milestones liefern Response-Snapshots.
- **Static analysis**: Enable nullable reference types everywhere (already on) + .NET analyzers set to `warning` in test projects to catch flaky patterns.
- **Data management**: Datenbankzustand wird über `tools/Croniq.DbMigrator` hergestellt (lokal via `dotnet run --project ... -- --connection`); Contract-Tests droppen nach jedem Lauf das Schema oder nutzen neue Container, um Bleed zu vermeiden.
- **Diagnostics**: Use `ITestOutputHelper` + structured logging to emit context (TenantId, ScheduleId). Contract/E2E suites push logs and traces to the Compose OTel Collector for triage.

## CI Pipelines

1. **PR pipeline** (GitHub Actions, workflow: `.github/workflows/tests.yml`):

- Stage `lint`: `dotnet format croniq.sln --verify-no-changes` keeps formatting drift from landing.
- Stage `build`: `dotnet restore` (with `actions/cache` for NuGet) + `dotnet build` (Release) for all solutions.

- Stage `unit-contract`: runs `dotnet test` for `Croniq.Core.Tests`, `Croniq.JobStore.InMemory.Tests`, `Croniq.Providers.Default.Tests`, and `Croniq.Observability.Tests` (observability runs here as a fast guard) with the shared Coverlet MSBuild props writing Cobertura output to `coverage/<Suite>/coverage.cobertura.xml`. After the last suite finishes we install `dotnet-reportgenerator-globaltool`, emit `coverage/report/Summary.json`, and keep both the per-suite XML and the aggregated summary as PR artifacts.
- Stage `artifacts`: publish `TestResults/*.trx`, `coverage/*.xml`, and docker logs emitted by contract suites.
- Gates: enforced inline (see `.github/workflows/tests.yml`): a tiny Python script reads `coverage/report/Summary.json` and fails the PR if `Croniq.Core` line coverage falls below 80% or if aggregate repository coverage drops under 70%.

2. **Nightly pipeline**:
   - Reuse PR template (matrix build) + add `docker-build` (push to GHCR `nuetzliches/croniq-nightly` with `:sha` tag).

- Stage `observability-telemetry`: run `dotnet test tests/Croniq.Observability.Tests/Croniq.Observability.Tests.csproj --configuration Release` before any Docker work so exporter regressions fail fast.
- Stage `e2e-compose`: invoke `scripts/test-e2e.cmd` (or replicate its steps) to build the stack, wait for `/health`, run the smoke project, and tear down. Collect Grafana/OTel traces and container logs as artifacts.
- Stage `security`: run `syft packages . -o json` (upload SBOM) + `trivy fs --exit-code 1 --severity HIGH,CRITICAL .`.

3. **Release pipeline**:
   - Triggered on tags `v*`.
   - Stage `full-test`: matrix of unit, contract, E2E (same as nightly but blocking).
   - Stage `package`: publish NuGet packages + container images with semver tags.
   - Stage `smoke-deploy`: deploy Helm/Compose to staging, run `tests/Croniq.Api.Smoke` against staging URL, collect logs, and provide rollback instructions referencing `CONCEPT.md` §17.

## Developer Workflow

- Use `dotnet test` locally with `--filter Category=Unit` or `=Contract` to target suites.
- Consult `tests/README.md` for quick commands, env variables, and troubleshooting tips covering every suite.
- For contract tests, ensure Docker Desktop (or another Docker runtime) is running. By default die SQL-Container-Fixture startet SQL Server 2022 und ruft `tools/Croniq.DbMigrator` auf, damit jede Suite mit frischem Schema/Seeds beginnt. Setze `CRONIQ_SQL`, um eine bestehende Datenbank zu nutzen (die Fixture führt Migration + Seeds trotzdem aus). Nutze `CaptureContainerLogsAsync()` nach Fehlschlägen, um SQL-Logs lokal zu sichern (CI automatisiert das später).
- Use `TestClock` when policy or scheduling logic relies on deterministic timestamps and the builders in `Croniq.TestKit.Builders` to create jobs/triggers without repeating boilerplate.
- For smoke tests, run `scripts\test-e2e.cmd`. It builds the Compose stack, waits for `/health`, runs `dotnet test tests/Croniq.Api.Smoke/Croniq.Api.Smoke.csproj --nologo`, and tears everything down. Override `CRONIQ_API_BASEURL`/`CRONIQ_API_KEY` before invoking the script when targeting remote environments (defaults remain `http://localhost:5080` and `smoke-key`).
- Document flaky scenarios immediately in `tests/README.md` (to be added) and open tracking issues.

## Backlog for the Testing Stream

- Delivered: `tests/Croniq.TestKit/` project with repository locator, DbMigrator runner, and eine SqlServer-Testcontainer-Fixture, die SQL Server 2022 startet (oder `CRONIQ_SQL` nutzt) und Default-Seeds setzt.
- Owners: Core + Persistence maintainers.
- [x] Extend `Croniq.TestKit` utilities (deterministic clock, payload builders, container log capture).
  - Delivered: `TestClock`, `JobDefinitionBuilder`, `TriggerDefinitionBuilder`, and `TestcontainerLogCollector` + Fixture-Hooks wie `CaptureContainerLogsAsync` für den Export von SQL-Logs.
  - Next: add response snapshot helpers + hook log export into CI artifacts.
- [x] Add FluentAssertions/NSubstitute across test projects and refactor existing tests for readability.
  - Deliverables: package references, shared assertions helpers, lint rule to forbid bare `Assert.True/False`.
  - Status: `Croniq.Persistence.SqlServer.Tests`, `Croniq.Core.Tests`, `Croniq.JobStore.InMemory.Tests`, and `Croniq.Providers.Default.Tests` migrated; extend to remaining suites.
- [x] Introduce `[Category]` traits and update `Directory.Build.props` to enforce Coverlet instrumentation.
  - Delivered: `TestCategories` helper + `[Trait]` annotations in contract suites and repository-level `Directory.Build.props` enabling automatic Coverlet output for every `*.Tests` project.
- [x] Create `Croniq.Api.Smoke` project + Compose file for automated end-to-end runs.
  - Delivered: `tests/Croniq.Api.Smoke/` HTTP harness exercising `/health` + `/schedules`, `infra/docker/docker-compose.tests.yml` wiring SQL + migrator + API + worker containers, `scripts/test-e2e.cmd` automation, and containerized sample hosts.
- [x] Publish developer guide (`docs/technical/testing.md` + `tests/README.md`) describing local setup, troubleshooting, and log collection.
  - Deliverables: new `tests/README.md` with quickstart + troubleshooting tree; update this doc after each milestone.
- [x] Wire GitHub Actions workflows (`.github/workflows/tests.yml`, `nightly.yml`) to run the described stages.
  - Deliverables: PR workflow covering lint/build/unit/contract, nightly workflow with compose E2E + scans, release workflow hooking into packaging + staging smoke tests.

By following this plan we can move the “Teststrategie” item in `CHECKLIST.md` from open to done once the outlined backlog is completed.
