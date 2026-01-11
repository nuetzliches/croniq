# Croniq Release & Compliance Playbook

This document extends `ci.md` with the steps every Croniq release must follow: versioning rules, artifact packaging, SBOM/signature evidence, and verification commands. It satisfies the docstreams backlog item "release-governance addendum".

## Versioning Policy

- Libraries/SDKs follow [SemVer](https://semver.org/). Breaking API changes require a new major version; additive changes increment minor; fixes increment patch.
- Services expose HTTP routes under `/v1`. Backwards-incompatible REST changes require `/v2` routes plus a deprecation window (minimum 2 release cycles).
- NuGet packages share the same version as the Git tag (e.g., `v0.6.0`). Docker images are tagged with both the semantic version and the commit SHA (`ghcr.io/...:0.6.0`, `ghcr.io/...:sha-<short>`).

### Automated versioning (MinVer)

- The solution uses [MinVer](https://github.com/adamralph/minver) (configured in `Directory.Build.props`) to derive `Version/PackageVersion` from Git tags.
- Tag prefix is `v` (e.g., `v1.2.3`). Release builds must be tagged; otherwise CI produces prereleases.
- Default prerelease identifiers are `preview.0` and `AutoIncrement` is `minor`, so commits after `v1.2.0` become `1.3.0-preview.<height>`.
- CI fetches full history (`fetch-depth: 0`) to ensure tags are available; local builds do the same automatically.
- Override only when necessary (e.g., hotfix branches): `dotnet build /p:MinVerVersionOverride=1.2.3`.

## Release Pipeline (GitHub Actions)

1. **Tagging** – create a signed tag `vX.Y.Z` on the main branch.
2. **Build phase**:
   - `dotnet build` + `dotnet test` (unit + contract suites).
   - Compose smoke tests (see `ci.md`) triggered for release candidates.
3. **Packaging**:
   - `dotnet pack` produces NuGet packages (Croniq.\*).
   - Docker images built with multi-stage Dockerfiles (`infra/docker`).
4. **Security evidence**:
   - Generate SBOM via Syft: `syft dir:artifacts/nuget -o spdx-json` plus image SBOMs from the built tags.
   - Scan images/packages with Trivy; fail on high-severity issues.
   - Sign container images with Cosign when keys are available. NuGet signing is gated and currently disabled until a public CA certificate is available.
5. **Publishing**:
   - Push NuGet packages with `dotnet nuget push`.
   - Push container images to GHCR.
6. **Verification**:
   - `cosign verify --certificate-identity <issuer> ghcr.io/...:0.6.0`.
   - `dotnet nuget verify Croniq.Core.0.6.0.nupkg --certificate-fingerprint <fingerprint>`.
   - Run `Croniq.DbMigrator` against the staging database (set `CRONIQ_SQL_CONNECTION` in the environment) to confirm migrations apply cleanly.

## Compliance Artifacts

| Artifact              | Location                            | Purpose                                      |
| --------------------- | ----------------------------------- | -------------------------------------------- |
| SBOM (`sbom.json`)    | Release artifacts / GH Actions      | Supply-chain inventory (SPDX JSON).          |
| Vulnerability reports | GH Actions logs / uploaded artifact | Evidence of Trivy/Snyk scans.                |
| Cosign bundles        | `ghcr.io` signatures                | Proof of signature (stored alongside image). |
| NuGet signatures      | Embedded in package                 | Validate authenticity via `nuget verify`.    |
| Test reports          | `artifacts/tests`                   | Document that unit/contract/smoke tests ran. |

## Rollback Strategy

- Database: forward-only migrations; rollback requires restoring backups (documented in `persistence.md`). Keep nightly backups for dev/test, point-in-time restore for production.
- Services: redeploy previous image tag (e.g., `ghcr.io/...:0.5.2`). Ensure migrations compatible before rolling forward again.

## Release Checklist

1. Update CHANGELOG/release notes if maintained; versioning comes from the git tag via MinVer.
2. Verify `Croniq.DbMigrator` runs locally with `CRONIQ_SQL_CONNECTION` set.
3. Run `npm --prefix docs run docs:build` to ensure docs compile; update navigation if new pages were added.
4. Tag and push; wait for CI to produce packages/images.
5. Download artifacts, run `cosign verify` + `nuget verify` locally if required by policy.
6. Publish release notes summarizing features, fixes, and any manual steps.

## Ownership & Automation Backlog

- Add a reusable GH Action for SBOM + signing to avoid duplicating steps across workflows.
- Automate release note generation from conventional commits (e.g., `git cliff`).
- Track release evidence in an internal registry for auditing if required.
