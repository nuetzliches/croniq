# Croniq Supply Chain & Release Security Plan

This plan details how we will fulfill the checklist item "SBOM/Signierung und Vulnerability Scans in Release-Flow einbauen". It complements the CI/CD (`ci.md`) and security (`security.md`) documents by focusing on artifact provenance, vulnerability management, and compliance evidence.

## Objectives

- Generate SBOMs for every distributable artifact (NuGet packages, container images) using an auditable toolchain.
- Perform automated vulnerability scans (dependencies + images) with gating thresholds before shipping.
- Sign packages and container images so consumers can verify authenticity.
- Persist scan/SBOM reports with release artifacts for traceability.

## SBOM Strategy

- **Tooling**: Use `syft` for SBOM generation (SPDX JSON) across source + built artifacts. Versions are defined in `eng/versions/supplychain-tools.json` and installed locally/CI via `scripts/ci/install-supplychain-tool.ps1 -Tool syft`, which places the binary in `bin/` and appends it to `PATH`.
- **NuGet packages**: After `dotnet pack`, run `syft packages ./artifacts/nuget -o spdx-json=sbom-nuget.json`.
- **Container images**: After Docker builds finish, run `syft ghcr.io/<owner>/croniq-<api|worker|webhooks|db-migrator>:<tag> -o spdx-json=sbom-api.json` (the release workflow already emits `api-<version>.spdx.json`, `worker-<version>.spdx.json`, `webhooks-<version>.spdx.json`, and `db-migrator-<version>.spdx.json`).
- **Storage**: Attach SBOM files to GitHub Releases and upload as workflow artifacts. Keep a copy under `artifacts/sbom/` in build output.

## Vulnerability Scanning

- **Dependencies**: `dotnet list package --vulnerable --include-transitive` in PR builds (warning) and release builds (fail on HIGH/CRITICAL unless waived).
- **Containers**: `trivy image ghcr.io/nuetzliches/croniq-api:<tag>` in release workflow; block on HIGH/CRITICAL. Trivy is installed through `scripts/ci/install-supplychain-tool.ps1 -Tool trivy`, sharing the same version manifest as Syft.
- **Source/FS**: `trivy fs --scanners vuln,secret .` nightly; fail on CRITICAL secrets/vulns.
- **Reports**: Upload SARIF to GitHub Security tab (`trivy ... -f sarif -o trivy.sarif`). Provide summary comment in PRs.
- **Waivers**: Maintain `docs/deep-dive/supplychain-waivers.md` documenting accepted risks, expiry dates, and references.

## License Compliance

- **Tooling**: Syft (SPDX JSON) plus `scripts/ci/check-licenses.py`. Allowed SPDX identifiers live in `eng/licenses/allowed-licenses.json`.
- **Policy**: The allow-list contains MIT and MIT-compatible SPDX IDs (MIT, MIT-0, Apache-2.0, BSD-2/3-Clause, ISC, CC0-1.0). `MS-EULA`, `LICENSE`, and `LICENSE.txt` are explicitly whitelisted because we manually reviewed the referenced packages (legacy .NET facades and Microsoft-provided shims) and verified they are redistributable within our policy scope. Additional exceptions require PRs that update the JSON files plus justification in this document.
- **Execution**: PR validation (`ci-pr.yml`), nightly compliance (`nightly.yml`) and the release workflow (`release.yml`) run `syft dir:. -o spdx-json=artifacts/licenses/sbom.json` followed by `python scripts/ci/check-licenses.py artifacts/licenses/sbom.json eng/licenses/allowed-licenses.json`. The SBOM and checker output are uploaded as evidence.
- **Fail-Fast Behavior**: Any package emitting a license identifier that is not on the allow list causes the job to fail. Contributors must either replace the dependency or document why it is still MIT-compatible and update the allow list.
- **Auditing**: The SBOM is diff-friendly and stored with build artifacts, enabling release reviewers to confirm the dependency set for each build.

## Waivers & Exceptions

- Temporary vulnerability waivers must follow `docs/deep-dive/supplychain-waivers.md` (expiry, mitigation, tracking issue). Reference waiver IDs in CI ignore lists where needed.
- Expired waivers cause CI/release to fail until resolved or renewed with security review.

## Toolchain Pinning & Local Usage

- Run `pwsh ./scripts/ci/install-supplychain-tool.ps1 -Tool syft` (or `-Tool trivy`) to download the pinned release declared in `eng/versions/supplychain-tools.json`. By default, binaries land in `./bin`; pass `-InstallDir` to override.
- After installation, prepend the resolved directory to `PATH` for the current shell (PowerShell example: `$env:Path = "$PWD/bin;$env:Path"`).
- Confirm the pinned versions with `./bin/syft --version` and `./bin/trivy --version` before running SBOM/scan commands locally.
- Mirror the CI experience when testing changes: `./bin/syft packages . -o cyclonedx-json` for SBOM validation and `./bin/trivy fs . --severity HIGH,CRITICAL --ignore-unfixed --exit-code 0` for informational vulnerability sweeps.
- CI jobs reuse the same script to ensure developers and automation execute identical bits. Updating either tool is a single-line version bump in the JSON manifest plus a follow-up validation run.
- The script supports Windows (zip) and Linux/macOS (tar.gz) archives and enforces amd64/arm64 builds. If another architecture is required later, extend the internal asset map.

## Signing & Provenance

- **NuGet**: Use `dotnet nuget sign` (or `nuget sign`) with an Azure Key Vault or local certificate; store certificate thumbprint in GitHub secret. Optional alternative: integrate with SignPath if available.
- **Containers**: Sign images using `cosign sign --key env://COSIGN_KEY ghcr.io/nuetzliches/croniq-api:<tag>`. Commit the public key at `infra/signing/cosign.pub` once generated so consumers can verify.
- **Public certs/keys**: Export the NuGet signing certificate as `infra/signing/nuget-signing.cer` and document its thumbprint (also referenced from `docs/SECURITY.md`).
- **Attestations**: Use `cosign attest` with predicate type `https://slsa.dev/provenance/v1` to link SBOM hash + build metadata.
- **Verification docs**: `docs/deep-dive/release-verification.md` contains consumer commands; `docs/SECURITY.md` summarizes guarantees.
- **Secrets in CI**: The release workflow looks for `COSIGN_KEY`, `NUGET_SIGNING_CERT_BASE64`, and `NUGET_SIGNING_CERT_PASSWORD` to enable signing steps.

### Provisioning runbook (manual, once per rotation)

1. **cosign key pair**
   - Generate locally: `cosign generate-key-pair --output-key cosign.key --output-pub cosign.pub`.
   - Store `cosign.key` as GitHub secret `COSIGN_KEY` (base64 contents), commit `infra/signing/cosign.pub`.
   - Optional: use `cosign generate-key-pair --kms <provider://key>` when moving to an HSM/KMS.
2. **NuGet signing cert**
   - For initial bootstrap, create a time-bounded code-signing cert (PowerShell):  
     `New-SelfSignedCertificate -Type CodeSigning -Subject "CN=Croniq NuGet Signing" -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddYears(1)`
   - Export to PFX (with password) and base64-encode to feed into GitHub secrets `NUGET_SIGNING_CERT_BASE64` and `NUGET_SIGNING_CERT_PASSWORD`.
   - Export the public CER and commit as `infra/signing/nuget-signing.cer`; note the thumbprint in `docs/SECURITY.md`.
   - When migrating to a managed CA/Key Vault, point `dotnet nuget sign` to the cert in the vault instead of the PFX workflow.
3. **Update docs**
   - Add new fingerprints/rotation notes to `docs/SECURITY.md`.
   - Keep `infra/signing/README.md` in sync with the current public artifacts.

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

- Maintain `docs/SECURITY.md` summarizing the guarantees and how users verify artifacts.
- Document secret provisioning (NuGet cert, cosign key) in an internal runbook referenced from this plan.
- Use GitHub environments with required reviewers for release workflow steps that access signing secrets.
- Track vulnerabilities via GitHub Security tab; triage SLA: CRITICAL <48h, HIGH <7d.

## Backlog to Complete the Checklist Item

- [x] Add `syft` and `trivy` to toolchain (`scripts/ci/install-supplychain-tool.ps1` + `eng/versions/supplychain-tools.json`) and document local usage. (2025-12-12)
- [x] Implement PR/nightly/release workflow steps for scans + SBOMs per the pipeline plan (see `.github/workflows/nightly.yml` + `.github/workflows/release.yml`).
- [ ] Provision signing keys (NuGet cert, cosign) and store public verification artifacts in the repo.
- [x] Add documentation (`docs/deep-dive/release-verification.md` + `SECURITY.md`) showing verification commands for consumers.
- [x] Create waiver process (template + file) for temporary vulnerability exceptions.
- [x] Ensure release workflow attaches SBOMs, scans, and signatures to GitHub Releases automatically.

Once this backlog is done, the checklist entry "SBOM/Signierung und Vulnerability Scans" can be marked complete.
