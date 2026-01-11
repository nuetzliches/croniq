# Croniq Testing Strategy

This document extends the quality vision captured in `architecture.md` and describes how every Croniq change is validated—from fast unit tests to release-day compliance checks. It is the single source of truth for contributors, QA, and release managers.

## Guiding Principles

- **Shift-left quality**: Favor fast feedback (unit + contract suites on every PR) while keeping higher-level validation on nightly/release pipelines.
- **Deterministic environments**: Every suite spins up the exact dependencies it needs (SqlServer containers, OTLP collectors, sample hosts) so failures are reproducible.
- **Observable failures**: Tests emit structured logs, metrics, and artifacts (TRX, Cobertura, container logs) to shorten mean time to resolution.
- **Shared tooling**: `Croniq.TestKit` provides fixtures, builders, trait constants, and diagnostics to avoid copy/paste test plumbing.

## Test Matrix (living reference)

| Suite                                                | Primary scope                                                                                                     | Trigger/Cadence             | Tooling / Infra                                         | Blocking rule                     |
| ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | --------------------------- | ------------------------------------------------------- | --------------------------------- |
| `Unit` (`tests/Croniq.*.Tests`)                      | Pure logic, options, schedulers, API surface guards                                                               | Every PR + local pre-push   | `xUnit`, `Shouldly`, `dotnet test`                      | Fail blocks merge                 |
| `Contract` (`*.ContractTests`)                       | Provider contracts (SqlServer persistence/auth, secrets) via Testcontainers                                       | Every PR (parallel)         | `Testcontainers`, seeded SQL, `Croniq.TestKit`          | Fail blocks merge                 |
| `Observability` (`tests/Croniq.Observability.Tests`) | Verifies OTLP exporter wiring using an in-memory collector + host builder                                         | Every PR + nightly + release | `dotnet test`, ASP.NET host, lightweight OTLP server    | Fail blocks merge/release         |
| `Smoke`/`E2E` (`tests/Croniq.Api.Smoke`)             | `Croniq.Sample.ApiHost` + `Croniq.Sample.WorkerHost` via Compose (InMemory auth, SqlServer persistence, migrator) | Nightly + release candidate | `scripts/test-e2e.cmd` (Docker Compose + `dotnet test`) | Fail blocks release/nightly badge |
| `Compliance`                                         | SBOM, Trivy scan, dependency audit                                                                                | Nightly + release           | `Syft`, `Trivy`, GitHub Actions reusable workflows      | Fail blocks release               |
| `Perf/Burn-in` (planned)                             | Long-running stress on scheduler leases + quotas                                                                  | On-demand / before GA       | Testcontainers + perf harness (to be defined)           | Informational                     |

## Suite Details

### Unit Tests

- **Scope**: Business rules in `src/*` projects (parsers, schedulers, policy resolvers, hosting extensions) with no I/O or network dependencies.
- **Frameworks**: `xUnit` + `Shouldly`, with `NSubstitute` for lightweight mocks when an interface cannot be satisfied with in-memory doubles.
- **Layout**: Mirror namespaces (e.g., `Croniq.Core.Tests/Scheduling/TriggerWorkerTests.cs`) with explicit Arrange–Act–Assert regions. Prefer `[Theory]` and `MemberData` for parser/policy matrices.
- **Execution**: `dotnet test tests/Croniq.Core.Tests/Croniq.Core.Tests.csproj --configuration Release` (or target another suite directly).
- **Gates**: Coverage per suite is aggregated by Coverlet; PRs fail when `Croniq.Core` line coverage <73%, overall line coverage <75%, or branch coverage <55% (overall + Core).

### Contract Tests

- **Scope**: Interactions with SqlServer-based persistence/auth providers and any boundary where Croniq talks to infrastructure or third-party services.
- **Frameworks**: `xUnit` + `Testcontainers for .NET`, orchestrated by `Croniq.TestKit`.
  - The SQL fixture starts `mcr.microsoft.com/mssql/server:2022` or reuses `CRONIQ_SQL` when provided.
  - Each run applies EF Core migrations via `tools/Croniq.DbMigrator` and seeds deterministic data.
  - `CreateProvider()` bootstraps `Croniq.Persistence.SqlServer` + `Croniq.Auth.SqlServer` with logging so tests can resolve `IJobPersistenceProvider`, `IApiKeyStore`, etc.
  - `TestCategories.Contract` standardizes `[Trait]` declarations, enabling `dotnet test --filter Category=Contract`.
  - `CaptureContainerLogsAsync`/`TestcontainerLogCollector` persist SQL logs for CI artifacts.
- **Layout**: Dedicated projects under `tests/*/*.ContractTests.cs`, e.g., `Croniq.Persistence.SqlServer.Tests/SqlServerJobPersistenceProviderTests.cs`.
- **Execution**: `dotnet test tests/Croniq.Persistence.SqlServer.Tests/Croniq.Persistence.SqlServer.Tests.csproj --filter Category=Contract`.
- **Gates**: Mandatory on every PR (parallel-friendly). Nightly runs add failover/concurrency cases.

### Observability Validation

- **Scope**: Ensures `AddCroniqObservability` wires OpenTelemetry for traces, metrics, and logs.
- **Framework**: `xUnit` plus a lightweight OTLP HTTP collector hosted via ASP.NET on a random port; tests emit one signal of each type and assert collector reception.
- **Execution**: `dotnet test tests/Croniq.Observability.Tests/Croniq.Observability.Tests.csproj`.
- **Cadence**: Every PR, nightly, and release pipelines.
- **Gate**: Failure blocks merge and release readiness because exporters would silently fail in production.

### End-to-End & Smoke Tests

- **Scope**: Validates that the sample API host, worker host, SqlServer persistence, and migrator collaborate successfully.
- **Frameworks**: `xUnit` harness under `tests/Croniq.Api.Smoke`, `Shouldly`, Docker Compose stack defined in `infra/docker/docker-compose.tests.yml`.
- **Execution**: `scripts\test-e2e.cmd` orchestrates build, compose up, readiness polling, `dotnet test tests/Croniq.Api.Smoke/...`, and teardown. Override `CRONIQ_API_BASEURL`/`CRONIQ_API_KEY` to target remote environments.
- **Cadence**: Nightly + release candidate builds; run manually before large API refactors. Failures block releases.
- **Scenarios**: `Webhook_ip_rule_crud_roundtrip` validates the management APIs, and `Webhook_ingress_respects_ip_rules` now hits the live ingress twice—first expecting `403 ip-blocked`, then `202 accepted` after adding a catch-all rule—alongside the existing health/schedule checks.
- **Configuration**: `CRONIQ_TENANT_ID`, `CRONIQ_ENVIRONMENT`, and `CRONIQ_WEBHOOK_BASEURL` let the suite point at non-default partitions and webhook hosts while sharing the same API base URL and key overrides.

### Compliance & Supply-Chain Checks

- **Scope**: SBOM generation, vulnerability scan, license audit.
- **Tools**: `syft packages . -o json`, `trivy fs/image --severity HIGH,CRITICAL`, future `cosign verify/sign` once signing keys land.
- **Cadence**: Nightly (informational) and release (blocking). Artifacts attach to the GitHub Release page for traceability.

### Performance & Burn-in (planned)

- **Scope**: Stress scheduler leases, rate-limit guards, and quota enforcement with long-running workloads.
- **Status**: Harness TBD (likely Testcontainers + dedicated worker host). Until GA these runs are informational but feed capacity planning.

## Coverage & Quality Gates

- Each `*.Tests` project inherits Coverlet MSBuild props from `Directory.Build.props`, emitting Cobertura files to `coverage/<Suite>/coverage.cobertura.xml`.
- `dotnet-reportgenerator-globaltool` consolidates coverage into `coverage/report/Summary.json`.
- Gates enforced in CI:
  - `Croniq.Core` line coverage ≥73% (hard fail).
  - Repository-wide line coverage ≥75%.
  - Branch coverage ≥55% (overall + `Croniq.Core`).
  - No PR merges when any unit/contract suite fails.
- Local guardrails: run `dotnet test --collect:"XPlat Code Coverage"` to preview coverage before opening a PR.

## Tooling & Shared Infrastructure

### Croniq.TestKit

- SqlServer container fixture with automatic migration + seed execution.
- Repository path resolver to locate sample data and compose files from any test project.
- Deterministic `TestClock`, payload/job builders, and canonical `TestCategories` constants.
- `CaptureContainerLogsAsync` helper exporting Docker logs to disk (CI attaches them as artifacts).

### Data & Environment Management

- `tools/Croniq.DbMigrator` keeps schemas consistent across suites; invoke via `dotnet run --project tools/Croniq.DbMigrator/Croniq.DbMigrator.csproj -- --connection <conn>` when reproducing issues.
- Default secrets and API keys come from sample hosts; never commit real credentials. Use `ISecretProvider` abstractions when tests require secret resolution.
- Docker Desktop (or another engine) must be running for contract/E2E suites; unit tests stay Docker-free.

### Diagnostics

- Tests log TenantId, ScheduleId, TriggerId alongside correlation IDs; this is required for triage.
- When SQL or Compose containers fail, inspect exported logs under `artifacts/containers/<suite>/` (CI) or the local folder returned by `CaptureContainerLogsAsync`.
- `tests/README.md` documents common failure signatures and suggested fixes.

## Local Developer Workflow

1. Install the .NET SDK 10.x (net10.0) plus Docker Desktop.
2. Restore tools: `dotnet tool restore`.
3. Run targeted suites:
   - Unit: `dotnet test tests/Croniq.Core.Tests/Croniq.Core.Tests.csproj`.
   - Contract: `CRONIQ_SQL=` optional override, then `dotnet test --filter Category=Contract` inside the desired project.
   - Observability: `dotnet test tests/Croniq.Observability.Tests/...`.
   - E2E: `scripts\test-e2e.cmd` (requires Docker + ports 5080/5081 free).
4. Inspect coverage locally: `dotnet test --collect:"XPlat Code Coverage"` followed by `reportgenerator` if deeper insight is needed.
5. Document flaky results immediately in `tests/README.md` and open a GitHub issue.

## CI Integration

### PR Workflow (`.github/workflows/ci-pr.yml`)

- **Lint**: `dotnet format croniq.slnx --verify-no-changes`.
- **Build**: `dotnet restore` + `dotnet build -warnaserror`.
- **Unit/Contract Matrix**: run `dotnet test` for each suite (Core, JobStore.InMemory, Providers.Default, Observability, Persistence.SqlServer contract tests) with Coverlet instrumentation.
- **Coverage Aggregation**: install `dotnet-reportgenerator-globaltool`, produce `coverage/report/Summary.json`, enforce thresholds.
- **Artifacts**: upload TRX, Cobertura XML, coverage summary, and container logs.

### Nightly Workflow (`.github/workflows/nightly.yml`)

- Reuses PR jobs.
- Adds Docker image build/push to `ghcr.io/nuetzliches/croniq-nightly:<sha>`.
- Runs Observability suite first to catch exporter regressions.
- Executes `scripts/test-e2e.cmd`, captures Grafana/OTel traces, stores Compose logs.
- Performs SBOM + Trivy scans (blocking), plus Markdown/link linting on `docs/`.

### Release Workflow (`.github/workflows/release.yml`)

- Trigger: tags `v*` or manual dispatch.
- Steps: full unit/contract/E2E suite, NuGet packing/publishing, multi-arch Docker buildx push, SBOM generation, `cosign` signing (when keys provisioned), Trivy image scan, staging smoke deploy (Compose or Helm) followed by `tests/Croniq.Api.Smoke` targeting staging URL.
- Outputs: Release notes (e.g., `git-cliff`), uploaded coverage + SBOM artifacts, signed container digests.

## Troubleshooting & Escalation

- **SQL container fails to start**: confirm Docker Desktop resources, inspect `artifacts/containers/sqlserver.log`, rerun with `CRONIQ_SQL` pointing to a local SQL instance if needed.
- **Coverage gate fails**: run `reportgenerator -reports coverage/**/coverage.cobertura.xml -targetdir coverage/report` locally to inspect per-file breakdown, add missing tests, rerun.
- **Intermittent E2E failure**: capture Compose logs (`docker compose logs`) before teardown by passing `KEEP_STACK=1` to `scripts/test-e2e.cmd`; open an issue with logs attached.
- **Category filters ignored**: ensure `[Trait(TestCategories.Category, TestCategories.Contract)]` is present and that `TestCategories` constants are referenced directly.
- Escalation path: post failures in `#croniq-alerts`, assign owners according to the backlog table below.

## Ownership & Backlog

- **Owners**: Core (unit coverage, schedulers), Persistence (contract tests, SQL fixtures), Platform (observability + CI), Release Engineering (compliance + signing), Solutions (E2E harness).
- **Delivered**: `Croniq.TestKit`, Shouldly/NSubstitute rollout, `[Category]` traits, Compose-based `Croniq.Api.Smoke`, this document + `tests/README.md`, GitHub Actions workflows for PR and nightly builds.
- **Open Backlog**:
  1. Hook container log export into CI artifacts automatically (partially implemented via `TestcontainerLogCollector`).
  2. Add response snapshot helpers to `Croniq.TestKit` for API regression detection.
  3. Flesh out performance/burn-in harness (decide on tooling, metrics, pass/fail criteria) before GA.
  4. Automate flaky-test quarantine workflow (label + GitHub issue template).

## Implementation Checklist

| Status | Item                                                         | Evidence / Next Step                                                                                                                                    |
| ------ | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| DONE   | Coverlet instrumentation enabled for every `*.Tests` project | `Directory.Build.props` sets `CollectCoverage=true` and wires `coverlet.msbuild`.                                                                       |
| DONE   | PR workflow runs lint/build/unit suites with coverage gates  | `.github/workflows/ci-pr.yml` executes all current unit suites, aggregates coverage, and enforces >=73% core / >=75% overall + >=55% branch thresholds. |
| DONE   | Observability suite covered in CI                            | `Run Croniq.Observability.Tests` step in `ci-pr.yml` validates OTLP wiring on every PR.                                                                 |
| DONE   | Smoke harness available for manual/Nightly use               | `scripts/test-e2e.cmd` orchestrates Docker Compose + `tests/Croniq.Api.Smoke`.                                                                          |
| DONE   | Croniq.TestKit shared fixtures committed                     | `tests/Croniq.TestKit/` now ships SQL Server fixtures, log collectors, and canonical trait constants.                                                   |
| DONE   | Contract suites for SqlServer providers run in CI            | `tests/Croniq.Persistence.SqlServer.Tests` exercises the EF provider and is wired into `.github/workflows/ci-pr.yml`.                                   |
| DONE   | Nightly workflow with Compose E2E + SBOM/Trivy               | `.github/workflows/nightly.yml` now runs the dev stack and a compliance job (Syft SBOM + Trivy scan) with blocking gates.                               |
| DONE   | Release workflow for packaging, signing, compliance          | `.github/workflows/release.yml` runs full tests, packs/publishes artifacts, builds images, and runs SBOM/Trivy scans.                                  |

Once these backlog items close, the “Teststrategie-Dokument” entry in `CHECKLIST.md` can move to **done**.
