# Croniq Supply Chain & Release Security Plan

This plan details how we will fulfill the checklist item "SBOM/Signierung und Vulnerability Scans in Release-Flow einbauen". It complements the CI/CD (`ci.md`) and security (`security.md`) documents by focusing on artifact provenance, vulnerability management, and compliance evidence.

## Objectives

- Generate SBOMs for every distributable artifact (NuGet packages, container images) using an auditable toolchain.
- Perform automated vulnerability scans (dependencies + images) with gating thresholds before shipping.
- Sign packages and container images so consumers can verify authenticity.
- Persist scan/SBOM reports with release artifacts for traceability.

## SBOM Strategy

- **Tooling**: Use `syft` for SBOM generation (SPDX JSON) across source + built artifacts. Pin version via `.config/dotnet-tools.json` or wrapper script.
- **NuGet packages**: After `dotnet pack`, run `syft packages ./artifacts/nuget -o spdx-json=sbom-nuget.json`.
- **Container images**: After Docker builds finish, run `syft ghcr.io/<owner>/croniq-<api|worker>:<tag> -o spdx-json=sbom-api.json` (the release workflow already emits `api-<version>.spdx.json` and `worker-<version>.spdx.json`).
- **Storage**: Attach SBOM files to GitHub Releases and upload as workflow artifacts. Keep a copy under `artifacts/sbom/` in build output.

## Vulnerability Scanning

- **Dependencies**: `dotnet list package --vulnerable --include-transitive` in PR builds (warning) and release builds (fail on HIGH/CRITICAL unless waived).
- **Containers**: `trivy image ghcr.io/nuetzliches/croniq-api:<tag>` in release workflow; block on HIGH/CRITICAL.
- **Source/FS**: `trivy fs --scanners vuln,secret .` nightly; fail on CRITICAL secrets/vulns.
- **Reports**: Upload SARIF to GitHub Security tab (`trivy ... -f sarif -o trivy.sarif`). Provide summary comment in PRs.
- **Waivers**: Maintain `SECURITY_NOTES.md` (future) documenting accepted risks, expiry dates, and references.

## Signing & Provenance

- **NuGet**: Use `dotnet nuget sign` (or `nuget sign`) with an Azure Key Vault or local certificate; store certificate thumbprint in GitHub secret. Optional alternative: integrate with SignPath if available.
- **Containers**: Sign images using `cosign sign --key env://COSIGN_KEY ghcr.io/nuetzliches/croniq-api:<tag>`. Store public key in repo (`infra/signing/cosign.pub`).
- **Attestations**: Use `cosign attest` with predicate type `https://slsa.dev/provenance/v1` to link SBOM hash + build metadata.
- **Verification docs**: Provide `docs/deep-dive/release-verification.md` (future) showing `cosign verify --key cosign.pub ...` steps.

## Workflow Integration

1. **PR (`ci-pr.yml`)**
   - `dotnet list package --vulnerable` (non-blocking warning).
   - `trivy fs --severity HIGH,CRITICAL --exit-code 0 .` (informational).
   - Publish summary comment if issues found.
2. **Nightly (`ci-nightly.yml`)**
   - Full `trivy fs` with `--exit-code 1` for CRITICAL/HIGH.
   - Generate SBOM for current main branch; upload artifact.
3. **Release (`release.yml`)**
   - Build packages/images, run the full test suite once more, and reuse the dependency vulnerability gate.
   - Generate SBOMs for NuGet artifacts (`syft dir:artifacts/nuget`) and GHCR images (direct image mode).
   - Execute `trivy fs` + `trivy image` with `exit-code 1` to block HIGH/CRITICAL exposures.
   - Sign NuGet packages (`dotnet nuget sign`) and container images (`cosign sign --key env://COSIGN_KEY`) whenever the secrets exist.
   - Attach SBOMs, SARIF scan reports, and signed artifacts to the GitHub Release (implemented in `.github/workflows/release.yml`). Attestations remain backlog work.

## Governance & Documentation

- Maintain `docs/SECURITY.md` (future) summarizing the guarantees and how users verify artifacts.
- Document secret provisioning (NuGet cert, cosign key) in an internal runbook referenced from this plan.
- Use GitHub environments with required reviewers for release workflow steps that access signing secrets.
- Track vulnerabilities via GitHub Security tab; triage SLA: CRITICAL <48h, HIGH <7d.

## Backlog to Complete the Checklist Item

- [ ] Add `syft` and `trivy` to toolchain (`.config/dotnet-tools.json` or scripts) and document local usage.
- [x] Implement PR/nightly/release workflow steps for scans + SBOMs per the pipeline plan (see `.github/workflows/nightly.yml` + `.github/workflows/release.yml`).
- [ ] Provision signing keys (NuGet cert, cosign) and store public verification artifacts in the repo.
- [ ] Add documentation (`docs/deep-dive/release-verification.md` + `SECURITY.md`) showing verification commands for consumers.
- [ ] Create waiver process (template + file) for temporary vulnerability exceptions.
- [x] Ensure release workflow attaches SBOMs, scans, and signatures to GitHub Releases automatically.

Once this backlog is done, the checklist entry "SBOM/Signierung und Vulnerability Scans" can be marked complete.
