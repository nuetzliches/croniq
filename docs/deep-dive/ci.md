# Croniq CI/CD Plan

This document describes the continuous integration and delivery strategy required to satisfy the "Build/Test CI Pipelines" and related checklist items. It builds on the testing, security, and observability plans.

## Goals

- **Deterministic builds**: `dotnet build` + tests run identically on developer machines and CI runners.
- **Fast feedback**: PR validation completes within ~10 minutes by parallelizing unit/contract suites.
- **Shift-left quality**: Formatting, analyzers, tests, coverage, and vulnerability scans run automatically.
- **Release readiness**: Nightly and release workflows build Docker images, NuGet packages, SBOMs, and signatures with traceable artifacts.

## Pipeline Topology

| Workflow         | Trigger                            | Purpose                                                                                                                                                        |
| ---------------- | ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ci-pr.yml`      | Pull request to `main`             | Lint, build, unit + contract tests with coverage, basic security checks.                                                                                       |
| `ci-nightly.yml` | Scheduled (UTC 02:00) + manual run | Full stack validation: PR steps + Compose E2E tests (dev stack), Docker image build, integration smoke, dependency scanning.                                   |
| `release.yml`    | Tag `v*` pushes or manual dispatch | Build & test release artifacts, publish NuGet packages and container images, gate on SBOM/vulnerability checks, sign assets, attach reports to GitHub Release. |

`ci-nightly.yml` and `release.yml` already live in `.github/workflows/` and can be triggered manually. The PR workflow (`ci-pr.yml`) remains outstanding (current PR validation runs via `tests.yml`).

## ci-pr.yml (Validation)

1. **Setup**
   - Runner: `ubuntu-latest` (use `windows-latest` matrix for MSSQL-specific tests if needed).
   - Checkout, restore tools (`dotnet workload restore` if required).
2. **Formatting & analyzers**
   - `dotnet format --verify-no-changes`.
   - `dotnet build -warnaserror` to enforce analyzers.
3. **Unit + contract tests**
   - Matrix per test project (`Croniq.Core.Tests`, `Croniq.JobStore.InMemory.Tests`, `Croniq.Persistence.SqlServer.Tests`, `Croniq.Providers.Default.Tests`).
   - Use `dotnet test <proj> /p:CollectCoverage=true /p:CoverletOutputFormat=cobertura`.
   - Upload coverage report aggregate + test results (TRX) as artifacts.
   - Fail if coverage <80% Core / <70% overall (use `coverlet.msbuild` thresholds).
4. **Security quick checks**
   - `dotnet list package --vulnerable --include-transitive`.
   - `trivy fs --exit-code 0 --severity HIGH,CRITICAL .` (informational in PR until release gating is ready).

## ci-nightly.yml (Full Suite)

- Inherits steps from `ci-pr` via reusable workflow `workflow_call` or composite action.
- Additional jobs:
  1. **Docker build + compose E2E**
     - Build images (`Croniq.Api`, worker) with BuildKit cache.
     - Run `docker compose -f infra/docker/docker-compose.yml -f infra/docker/docker-compose.dev.yml -f infra/docker/docker-compose.observability.yml --profile api --profile worker --profile obs up --build -d` (aligned with `scripts/devstack-up.cmd`).
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
   - `images` job builds the API + Worker sample hosts using the Dockerfiles under `samples/`, tags/pushes them to `ghcr.io/<owner>/croniq-{api|worker}:<tag>` plus `:latest`, and creates SBOMs directly from the pushed images.
5. **Security & Compliance**
   - `trivy fs` runs before packaging to gate dependency vulnerabilities, `trivy image` scans each GHCR image, and SBOMs + SARIF reports upload as workflow artifacts. Cosign signing is executed when `COSIGN_KEY`/`COSIGN_PASSWORD` secrets are available.
6. **Release Publishing**
   - The `publish` job downloads all artifacts, collates SBOMs/scan results, and attaches them to the GitHub Release produced by the tag (manual dispatch reuses the same mechanism). Smoke deploys remain a backlog item once staging infrastructure is ready.

## Tooling & Repo Layout

- Add `.github/workflows/` with the three YAML workflows.
- Define composite actions or scripts under `scripts/ci/` for reuse (e.g., `scripts/ci/run-tests.ps1`).
- Provide `Directory.Build.props` enabling analyzers and coverlet instrumentation defaults.
- Use `.config/dotnet-tools.json` to pin CLI tools (dotnet-format, coverlet, gitversion, etc.).
- Add `eng/` directory for common pipelines assets (templates, env files).

## Secrets & Environment

- Store secrets (NuGet API key, registry tokens, cosign key, staging kubeconfig) in GitHub Actions secrets.
- Use environments for release deployments with required reviewers.
- Provide `scripts/ci/setup-sql.ps1` invoked by workflows needing SQL Server (e.g., contract tests) — use Azure SQL Edge container for Linux runners.

## Backlog to Complete CI/CD Milestone

- [ ] Create `.github/workflows/ci-pr.yml` implementing the described validation stages (nightly + release workflows already added).
- [ ] Add reusable composite actions or scripts for test execution, coverage aggregation, and Compose orchestration.
- [ ] Check in `Directory.Build.props`, `.config/dotnet-tools.json`, and `eng/` helpers referenced by the workflows.
- [ ] Document local reproduction steps (`docs/deep-dive/ci.md` + `README`) so developers can mimic CI commands.
- [ ] Configure required secrets/environments in GitHub with least privilege and document them in an internal runbook.
- [ ] Hook coverage & test results into PR status (e.g., Codecov or built-in summary comment).
- [x] Integrate SBOM + signing steps into the release workflow (cosign execution becomes active once secrets are provided).

Once these backlog items land, the "Build/Test CI Pipelines" checklist entry can move to done.
