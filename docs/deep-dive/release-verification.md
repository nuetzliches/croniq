# Croniq Release Verification Guide

This guide shows how consumers can verify Croniq release artifacts (containers and NuGet packages) using the published signing materials and scan evidence.

## Prerequisites

- Install `cosign` (same version as CI, see `eng/versions/supplychain-tools.json` or use `scripts/ci/install-supplychain-tool.ps1 -Tool cosign` once added).
- Install .NET SDK (for `dotnet nuget verify`).
- Download the release assets (SBOMs, scan reports, signed packages/images) from GitHub Releases.

## Public Signing Material

- Container images: `infra/signing/cosign.pub` (cosign public key, commit this once generated).
- NuGet packages: `infra/signing/nuget-signing.cer` (exported signing certificate), plus its thumbprint listed below the file in the repo once available.

## Verify container images

Replace `<owner>` with the GitHub org/user and `<tag>` with the release tag (e.g., `v1.2.3`):

```bash
cosign verify \
  --key infra/signing/cosign.pub \
  ghcr.io/<owner>/croniq-api:<tag>

cosign verify \
  --key infra/signing/cosign.pub \
  ghcr.io/<owner>/croniq-worker:<tag>
```

Notes:
- If the key is not yet published, verification will fail; do not deploy unsigned images.
- The release assets include SBOMs (`api-<tag>.spdx.json`, `worker-<tag>.spdx.json`) and Trivy reports. Compare `cosign verify` output with the image digests listed on the Release page.

## Verify NuGet packages

Assuming `infra/signing/nuget-signing.cer` exists and contains the public cert used in the release workflow:

```bash
dotnet nuget verify artifacts/nuget/Croniq.Core.<version>.nupkg \
  --signature-verification-mode require \
  --certificates infra/signing/nuget-signing.cer
```

Use the same command for other Croniq packages. If you prefer fingerprints, add `--certificate-fingerprint <THUMBPRINT>` using the value documented next to the certificate file.

## Inspect SBOMs and scan evidence

- SBOMs: Compare the attached SPDX files against the images or packages you consume (`syft packages ghcr.io/<owner>/croniq-api:<tag> -o spdx-json` should yield the same package set).
- Vulnerability scans: Review `trivy-image-api.sarif` / `trivy-image-worker.sarif` and `trivy-fs.sarif` attached to the release. The release gate blocks CRITICAL/HIGH unless a waiver exists.
- License scan: `artifacts/licenses/license-scan.json` is attached to the release; it must show only allow-listed SPDX IDs.

If any verification step fails, halt deployment and open an incident/issue referencing the failing artifact and command.***
