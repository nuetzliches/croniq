# Security & Supply Chain Guarantees

This document summarizes how Croniq secures releases and how consumers can verify them. Detailed procedures live in `docs/deep-dive/security.md`, `docs/deep-dive/supplychain.md`, and `docs/deep-dive/release-verification.md`.

## What we ship

- Signed container images (`croniq-sample-api`, `croniq-sample-worker`) published to GHCR with cosign.
- Signed NuGet packages published from the release workflow.
- Attached SBOMs (SPDX), Trivy scan reports (filesystems + images), and license scan output for every release.

## Public keys / fingerprints

- cosign public key: `infra/signing/cosign.pub` (commit in repo; use with `cosign verify`).
- cosign key ID (SHA256 of public key PEM): `254a3966edb86ce966a6a2d81fc56011d833b386fc22a4ed1657e875332e2fca`
- NuGet signing certificate: `infra/signing/nuget-signing.cer`
  - Thumbprint: `64FAE63096D184E8C4E8710A59175F3D734FCBB0`

## How to verify

- Containers: `cosign verify --key infra/signing/cosign.pub ghcr.io/<owner>/croniq-sample-api:<tag>` (and `croniq-sample-worker`). See `docs/deep-dive/release-verification.md` for step-by-step commands.
- NuGet: `dotnet nuget verify <package>.nupkg --certificates infra/signing/nuget-signing.cer --signature-verification-mode require`.
- SBOM/Scans: Compare attached SPDX files and Trivy SARIF reports with the artifacts you consume.

## Reporting security issues

- For private disclosures, email the maintainers or open a security advisory in GitHub (preferred once the repo is public).
- Include version, artifact, and reproduction steps; avoid sharing secrets or personal data in issue text.

## Waivers and exceptions

- Temporary vulnerability waivers are tracked in `docs/deep-dive/supplychain-waivers.md` with expiry dates and mitigations.
- CI/release gates fail when waivers expire or when new HIGH/CRITICAL issues are discovered without an approved waiver.***
