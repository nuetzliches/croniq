# Croniq CI/CD Plan

This document describes the continuous integration and delivery strategy required to satisfy the "Build/Test CI Pipelines" and related checklist items. It builds on the testing, security, and observability plans.

## Goals

- **Deterministic builds**: `dotnet build` + tests run identically on developer machines and CI runners.
- **Fast feedback**: PR validation completes within ~10 minutes by parallelizing unit/contract suites.
- **Shift-left quality**: Formatting, analyzers, tests, coverage, and vulnerability scans run automatically.
- **Release readiness**: Nightly and release workflows build Docker images, NuGet packages, SBOMs, and signatures with traceable artifacts.

## Pipeline Topology

| Workflow             | Trigger                                | Purpose                                                                                                                                                        |
| -------------------- | -------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ci-pr.yml`          | Pull request to `main`                 | Lint, build, unit + contract tests with coverage, basic security checks.                                                                                       |
| `nightly.yml`        | Scheduled (UTC 02:00) + manual run     | Full stack validation: PR steps + Compose E2E tests (dev stack), Docker image build, integration smoke, dependency scanning.                                   |
| `release.yml`        | Tag `v*` pushes or manual dispatch     | Build & test release artifacts, publish NuGet packages and container images, gate on SBOM/vulnerability checks, sign assets, attach reports to GitHub Release. |
| `deploy-staging.yml` | Manual (`workflow_dispatch`) + staging | Helm deploy Croniq to the staging cluster, run HTTPS health probes, execute smoke tests against the staging ingress, and collect Kubernetes diagnostics.       |
| `dacpac.yml`         | Manual (`workflow_dispatch`) + guard   | Provision Azure SQL Edge locally and publish a DACPAC for schema validation; jobs stay skipped until `run_workflow` is set to true.                            |

`nightly.yml`, `ci-pr.yml`, and `release.yml` already live in `.github/workflows/` and can be triggered manually (the former `tests.yml` workflow has been retired).

## ci-pr.yml (Validation)

1. **Setup**
   - Runner: `ubuntu-latest` (use `windows-latest` matrix for MSSQL-specific tests if needed).
   - Checkout, restore tools (`dotnet workload restore` if required).
2. **Formatting & analyzers**
   - `dotnet format --verify-no-changes`.
   - `dotnet build -warnaserror` to enforce analyzers.
3. **Unit + contract tests**
   - Matrix per test project (`Croniq.Core.Tests`, `Croniq.Api.Tests`, `Croniq.JobStore.InMemory.Tests`, `Croniq.Persistence.SqlServer.Tests`, `Croniq.Providers.Default.Tests`, `Croniq.Sdk.Tests`).
   - Use `dotnet test <proj> /p:CollectCoverage=true /p:CoverletOutputFormat=cobertura`.
   - Upload coverage report aggregate + test results (TRX) as artifacts.
   - Fail if coverage <73% Core line / <75% overall line / <55% branch (overall + Core).
4. **Security quick checks**
   - `dotnet list package --vulnerable --include-transitive`.
   - `trivy fs --exit-code 0 --severity HIGH,CRITICAL .` (informational in PR until release gating is ready).
5. **Status reporting**
   - `scripts/ci/enforce_coverage_thresholds.py` enforces the line + branch thresholds above.
   - The workflow posts an auto-updating PR comment summarizing overall + Croniq.Core coverage plus per-assembly breakdown.

## nightly.yml (Full Suite)

- Inherits steps from `ci-pr` via reusable workflow `workflow_call` or composite action.
- Additional jobs:
  1. **Docker build + compose E2E**
     - Build images (`croniq-api`, `croniq-worker`, `croniq-webhooks`, `croniq-db-migrator`) with BuildKit cache.
     - Use `scripts/ci/compose-devstack.ps1 -Action Up` to start the stack (wraps the `docker compose -f ... --profile api|worker|obs up --build -d` invocation shared with `scripts/devstack-up.cmd`).
     - Execute `tests/Croniq.Api.Smoke` suite against the stack.
     - Collect logs from containers, upload as artifacts.
  2. **Observability verification**
     - Start OTel collector service container and run a lightweight probe to ensure OTLP export works.
  3. **Dependency + license scan**
     - `trivy fs --exit-code 1 ...` (enforce), `syft . -o json > sbom.json`.
  4. **Docs validation**
     - Broken link checker over `docs/` (e.g., `lychee`), ensure Markdown lint passes.

## release.yml (Artifacts & Deploy)

1. **Versioning**
   - Uses the pushed git tag (`vMAJOR.MINOR.PATCH`) by default; manual dispatch can override via the `version` input. Tags feed directly into NuGet package metadata and GHCR image tags.
2. **Build & Test**
   - `tests` job restores, builds (Release config), and runs the full solution test suite once more for release traceability. TRX logs upload as artifacts for auditing.
3. **Package Publishing**
   - `packages` job executes `dotnet pack`, generates SBOMs with `syft dir:artifacts/nuget -o spdx-json`, runs `dotnet list package --vulnerable --include-transitive`, signs `.nupkg` files when signing secrets exist, and optionally pushes to NuGet.org via `NUGET_API_KEY`.
4. **Container Images**
   - `images` job builds the production hosts via `infra/docker/Dockerfile.production`, tags/pushes them to `ghcr.io/<owner>/croniq-{api|worker|webhooks|db-migrator}:<tag>` plus `:latest`, and creates SBOMs directly from the pushed images.
5. **Compose Smoke Verification**
   - `smoke` job uses `scripts/ci/compose-devstack.ps1` to spin up the API/worker/observability stack, waits for `/health`, runs `tests/Croniq.Api.Smoke`, and uploads collected logs before teardown.
6. **Security & Compliance**
   - `trivy fs` runs before packaging to gate dependency vulnerabilities, `trivy image` scans each GHCR image, and SBOMs + SARIF reports upload as workflow artifacts. Cosign signing is executed when `COSIGN_KEY`/`COSIGN_PASSWORD` secrets are available.
7. **Release Publishing**
   - The `publish` job downloads all artifacts, collates SBOMs/scan results, and attaches them to the GitHub Release produced by the tag (manual dispatch reuses the same mechanism).
8. **Staging Deployment (workflow_call)**
   - After `smoke` succeeds, `release.yml` reuses `deploy-staging.yml` via `workflow_call` (passing the release tag) to push the Helm chart to the staging cluster automatically. `publish` depends on this job so GA artifacts are only published after staging health + smoke verification pass.

### deploy-staging.yml (Helm Deploy)

- Trigger: manual `workflow_dispatch` guarded by the `staging` environment **and** `workflow_call` from `release.yml`.
- Prereqs: `charts/croniq` + `charts/environments/staging-values.yaml`, secrets `STAGING_KUBECONFIG` (base64), optional `image-tag` input (release passes the git tag).
- Steps:
  1. Resolve image tag (defaults to current ref, workflow input, or `staging-<run>`).
  2. Configure kubectl/Helm with the staging kubeconfig and install/upgrade the chart with staged API/worker tags.
  3. Run layered health probes via `scripts/ci/wait-for-http.ps1` against `/health`, `/health/persistence`, and `/webhooks/health`; on failure, capture `kubectl get/describe/logs` before exiting.
  4. Execute `dotnet test tests/Croniq.Api.Smoke/... -- TestRunParameters.Parameter(BaseUrl, https://staging.croniq.local)` to validate ingress + APIs.
  5. Collect diagnostics (pods, describe output, API/worker logs) as artifacts for traceability.

Release builds automatically call this workflow; you can still dispatch it manually for ad-hoc staging refreshes.

### dacpac.yml (Manual SQL Deploy, Disabled by Default)

- Triggered only via `workflow_dispatch` and gated by the `run_workflow` boolean input (defaults to `false`). Unless you explicitly toggle it to `true`, the job is a no-op—this keeps the workflow checked in but effectively disabled.
- Installs `sqlpackage` via a .NET global tool, provisions Azure SQL Edge through `scripts/ci/setup-sql.ps1`, and immediately executes `scripts/ci/deploy-dacpac.ps1` with the provided DACPAC/database inputs.
- Accepts inputs for container name, host port, database, DACPAC path, and Compose vs. single-container provisioning so you can mirror local layouts. Update the defaults if artifacts move.
- Optionally set the `SQL_EDGE_SA_PASSWORD` repository secret to override the default `sa` password when running the workflow; the script falls back to `P@ssw0rd1234` otherwise.
- Use this workflow when you need a manual, audit-friendly way to prove DACPAC compatibility (e.g., before enabling the SQL-dependent test suites in CI) without turning on Azure SQL resources.

### Rollback & Recovery

1. Inspect deployment history: `helm history croniq -n croniq-staging`.
2. Roll back to a prior revision: `helm rollback croniq <REVISION> -n croniq-staging` (the workflow artifacts include the revision deployed for each release run).
3. Alternatively, rerun `deploy-staging.yml` with `image-tag` set to the previous release tag.
4. After rollback, re-run `scripts/ci/wait-for-http.ps1` (both `/health` and `/health/persistence`) and `dotnet test tests/Croniq.Api.Smoke` with the staging BaseUrl to confirm stability.
5. Capture fresh diagnostics and attach them to the incident ticket/runbook for auditability.

## Tooling & Repo Layout

- Add `.github/workflows/` with the three YAML workflows.
- Define reusable helpers in `scripts/ci/` (e.g., `scripts/ci/run-tests.ps1` for dotnet suites, `scripts/ci/compose-devstack.ps1` for Compose orchestration).
- Provide `Directory.Build.props` enabling analyzers and coverlet instrumentation defaults.
- Use `.config/dotnet-tools.json` to pin CLI tools (dotnet-format, coverlet, gitversion, etc.).
- Add `eng/` directory for common pipelines assets (templates, env files).

## Local Reproduction Quickstart

1. **PR test matrix locally**
   - `pwsh ./scripts/ci/run-tests.ps1 -Project tests/Croniq.Core.Tests/Croniq.Core.Tests.csproj -DisplayName "Croniq.Core.Tests"`
   - Repeat per suite; artifacts land under `TestResults/` + `coverage/` just like CI.
2. **Coverage summary + gates**
   - Run `reportgenerator "-reports:coverage/**/coverage.cobertura.xml" "-targetdir:coverage/report" -reporttypes:JsonSummary`.
   - Enforce thresholds with `python scripts/ci/enforce_coverage_thresholds.py coverage/report/Summary.json --core-assembly Croniq.Core --core-threshold 73 --overall-threshold 75 --core-branch-threshold 55 --overall-branch-threshold 55` (same logic as CI).
3. **Compose-driven dev stack**
   - `pwsh ./scripts/ci/compose-devstack.ps1 -Action Up` starts the API/worker/observability profiles.
   - `pwsh ./scripts/ci/compose-devstack.ps1 -Action Down -CaptureLogs` stops the stack and collects logs to `artifacts/ci-compose/`.
4. **HTTP health probes**
   - `pwsh ./scripts/ci/wait-for-http.ps1 -Uri http://localhost:5080/health` blocks until the endpoint returns 200 (mirrors the nightly/release workflows).

## Secrets & Environment

| Workflow             | Environment                    | Required Secrets                                                                                                                   | Notes                                                                             |
| -------------------- | ------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `ci-pr.yml`          | _none_                         | (optional) `CODECOV_TOKEN` once coverage uploads are enabled                                                                       | Uses repo-level permissions only.                                                 |
| `nightly.yml`        | `nightly` (optional)           | `GITHUB_TOKEN` (default)                                                                                                           | Additional secrets only needed for experimental scans.                            |
| `release.yml`        | `release`                      | `NUGET_API_KEY`, `NUGET_SIGNING_CERT_BASE64`, `NUGET_SIGNING_CERT_PASSWORD`, `COSIGN_KEY`, `COSIGN_PASSWORD`, `STAGING_KUBECONFIG` | `STAGING_KUBECONFIG` is forwarded to `deploy-staging.yml` via `workflow_call`.    |
| `deploy-staging.yml` | `staging` (requires reviewers) | `STAGING_KUBECONFIG`                                                                                                               | kubeconfig is base64-encoded; workflow decodes to `kubeconfig`.                   |
| `dacpac.yml`         | _none_                         | (optional) `SQL_EDGE_SA_PASSWORD`                                                                                                  | Manual workflow remains disabled until the `run_workflow` input is set to `true`. |

Reference `eng/pipelines/secrets.template.md` when provisioning secrets so the internal runbook stays in sync.

- Store secrets (NuGet API key, registry tokens, cosign key, staging kubeconfig) in GitHub Actions secrets and rotate them regularly.
- Use environments for release/staging so deployments require manual approval.
- Use `scripts/ci/setup-sql.ps1` when CI or local dev needs an Azure SQL Edge instance for contract tests (defaults to a docker container on port 1433, can switch to compose via `-UseDockerCompose`).
- Provide a DACPAC when you need schema deployment: `pwsh ./scripts/ci/setup-sql.ps1 -DacpacPath artifacts/db/Croniq.dacpac -Database CroniqLocal -HostPort 14330`. The script forwards parameters to `scripts/ci/deploy-dacpac.ps1`, so the DACPAC publish happens immediately after the container is healthy. Add `-AllowDataLoss` when intentionally running destructive migrations.

## Backlog to Complete CI/CD Milestone

- [x] Create `.github/workflows/ci-pr.yml` implementing the described validation stages (nightly + release workflows already added).
- [x] Add reusable composite actions or scripts for test execution, coverage aggregation, and Compose orchestration (`scripts/ci/run-tests.ps1`, `scripts/ci/enforce_coverage_thresholds.py`, `scripts/ci/compose-devstack.ps1`).
- [x] Check in `Directory.Build.props`, `.config/dotnet-tools.json`, and `eng/` helpers referenced by the workflows.
- [x] Document local reproduction steps (`docs/deep-dive/ci.md` + contributor guidance) so developers can mimic CI commands.
- [x] Configure required secrets/environments in GitHub with least privilege and document them in an internal runbook (`eng/pipelines/secrets.template.md`).
- [x] Hook coverage & test results into PR status (auto-updating coverage summary comment in `ci-pr.yml`).
- [x] Integrate SBOM + signing steps into the release workflow (cosign execution becomes active once secrets are provided).
- [x] Stand up `deploy-staging.yml` Helm workflow (see "deploy-staging.yml" section) to deploy the staging cluster via Helm.

All backlog items are complete; the "Build/Test CI Pipelines" checklist entry can move to done.
