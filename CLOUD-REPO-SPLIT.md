# Cloud Repo Split Sketch (Future)

This file is intentionally forward-looking and can be deleted later.
It describes a plausible way to split self-hosted Croniq (V1) from hosted/cloud-specific work.

## Goals

- Keep the **self-hosted V1** repo focused (docs + code + tests).
- Allow hosted/cloud work to evolve on a different cadence without constantly polluting V1 docs.
- Preserve a single set of contracts for:
  - scheduler semantics,
  - job keys,
  - persistence abstractions,
  - API/gRPC payloads.

## Recommended split (when needed)

### Repo 1: `croniq` (this repo)

- Source of truth for the **product core**:
  - `Croniq.Core`, `Croniq.Sdk`, `Croniq.Hosting`
  - `Croniq.Persistence.*` providers (InMemory/SqlServer)
  - `Croniq.Api` (self-hosted management surface)
  - `Croniq.Webhooks`
- Docs remain primarily self-hosted oriented.
- Tests stay authoritative for core semantics.

### Repo 2: `croniq-cloud` (future)

- Cloud-only concerns:
  - provisioning and tenant onboarding automation
  - operator portal / hosted admin surface
  - cloud control-plane services (if they exist)
  - hosted operational tooling, SRE runbooks
  - multi-environment/multi-region orchestration
- Strong bias towards **composition** over forks:
  - consume `croniq` packages as NuGet dependencies
  - avoid copying shared domain logic

## Package / versioning strategy

### Option A (recommended): publish NuGet from `croniq`

- `croniq` produces versioned packages (SemVer) for the pieces `croniq-cloud` needs:
  - `Croniq.Core`
  - `Croniq.Sdk`
  - `Croniq.Hosting`
  - `Croniq.Auth.Abstractions` / `Croniq.Persistence.Abstractions`
  - `Croniq.Rpc.Client`
- `croniq-cloud` pins versions and upgrades via normal dependency management.

**Pros**: clean boundaries, reproducible builds, no git coupling.

**Cons**: requires disciplined API stability and release flow.

### Option B: shared contracts repo (only if necessary)

- Extract only pure contracts into a tiny repo (e.g., `croniq-contracts`):
  - DTOs, gRPC protos, shared claim names, versioning policy.

**Pros**: very small surface.

**Cons**: extra repo + CI overhead; easy to over-extract.

## Boundary rules (to prevent churn)

- `croniq-cloud` should not depend on `Croniq.Api` internals; it should depend on:
  - public HTTP/gRPC contracts
  - published SDK/client libraries
- Cloud-only features should be introduced in `croniq` only if they are also valuable for self-hosted.
- Anything that is primarily operational/hosted-specific goes into `croniq-cloud`.

## Migration trigger (when to split)

Split is usually justified when at least one becomes true:

- cloud-only deployment/provisioning code starts to dominate PRs
- cloud-only dependencies appear that are not acceptable for the self-hosted repo
- different release cadence is required (rapid portal iterations vs stable scheduler)
- docs are repeatedly fighting between self-hosted and hosted narratives

## Minimal migration plan

1. Lock down the public package boundaries in `croniq` (what is exported as NuGet).
2. Ensure `croniq-cloud` can run against released packages (no source sharing).
3. Move cloud-only docs/runbooks into `croniq-cloud` (or keep them out-of-tree).
4. Keep this file as a temporary note; delete once the split is real and documented in the new repo.
