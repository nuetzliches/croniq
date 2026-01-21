# Croniq Aspire Devstack Concept

_Last updated: 2026-01-21_

## Objectives

- Replace the current devstack PowerShell/CMD scripts with a .NET Aspire app host.
- Keep local onboarding one-command and predictable (API + worker + database + optional observability).
- Preserve the existing configuration contract (CRONIQ_* envs, .env semantics) while we transition.
- Make the devstack ready for gRPC-Web via a local TLS proxy (Caddy preferred).

## Decisions

- Use `tools/Croniq.Devstack.AppHost` (net10.0) as the Aspire app host location and keep it in the solution.
- Keep `.env` as a supported configuration source; process env vars override file values.
- Prefer Aspire for local development; keep Docker Compose for CI until parity is proven.
- Keep the Angular UI as a manual `npm start` step for now; revisit once the AppHost stabilizes.
- Run `Croniq.Sample.Dmz` in the AppHost by default (always on).

## Current Devstack Baseline (Must Keep Working)

- Docker Compose stack: `infra/docker/docker-compose.yml`, `docker-compose.dev.yml`, `docker-compose.observability.yml`.
- Scripts orchestrate:
  - SQL Server container + `croniq-db-migrator` first, then API/worker profiles.
  - Optional host-run `Croniq.Sample.ApiHost` + `Croniq.Sample.Dmz`.
  - Optional UI (`npm start`) in a separate terminal.
  - Health checks for `/health`, optional log following, PID cleanup on shutdown.
- Profiles used today: `api`, `worker`, `obs`.

## Proposed Aspire Topology

- AppHost project `tools/Croniq.Devstack.AppHost` (net10.0).
- Resources:
  - SqlServer container (default), optional Postgres container/attachment.
  - `Croniq.DbMigrator` as a project resource that runs before API/worker.
  - `Croniq.Sample.ApiHost` and `Croniq.Sample.WorkerHost` as project resources.
  - `Croniq.Sample.Dmz` always on (for webhook relay dev scenarios).
  - Observability containers (otel-collector, grafana, tempo, prometheus) behind an `obs` toggle.
  - Caddy reverse proxy for TLS termination and gRPC-Web endpoints (preferred).
- Aspire dashboard is kept, but Grafana/Tempo/Prometheus stay available to match existing docs.

## Configuration Strategy

- Keep CRONIQ_* env variable support and map into resources via `WithEnvironment` and `WithReference`.
- Keep `.env` as the local source of truth; process env vars override file values.
- Centralize port assignments in the AppHost to avoid per-script duplication.

## Migration Strategy (High-Level)

1. Add AppHost in parallel to the existing devstack (no breaking change).
2. Update docs to prefer Aspire for local dev; keep Compose for CI until parity is proven.
3. Deprecate and remove `scripts/devstack-*.ps1` + `scripts/devstack-*.cmd`.

## Checklist

### Planning & Docs
- [x] Capture Aspire design decisions in `docs/deep-dive/devstack.md` and link from `docs/deep-dive/architecture.md`.
- [x] Define the intended AppHost location/name and solution structure (net10.0).
- [x] Decide whether `.env` remains supported or migrates to user-secrets/appsettings.

### AppHost Foundation
- [x] Create the Aspire AppHost project (net10.0) with service defaults.
- [x] Wire SQL Server container + `Croniq.DbMigrator` run-before dependency.
- [x] Add API and Worker project resources with the same env vars as the Compose stack.
- [x] Add DMZ project resource for webhook relay scenarios.

### Observability & Tooling
- [x] Integrate the observability containers (otel-collector, grafana, tempo, prometheus) behind an `obs` toggle.
- [x] Align OTLP ports and tenant headers with existing observability config.
- [x] Document how Aspire dashboard coexists with Grafana/Tempo/Prometheus.

### TLS / gRPC-Web (Caddy)
- [ ] Add a Caddy container resource (or sidecar) for local TLS termination.
- [ ] Provide a Caddyfile for routing API + gRPC-Web endpoints (and UI if needed).
- [ ] Document local trust steps (`caddy trust`) and default URLs.

### UI Integration
- [ ] Decide whether the Angular UI is launched by Aspire or remains manual.
- [ ] If Aspire-managed: add a Node/NPM resource with health checks + ports.

### Back-Compat & Cleanup
- [ ] Keep Compose-based CI (`scripts/ci/compose-devstack.ps1`) until Aspire parity.
- [ ] Deprecate `scripts/devstack-up.cmd`, `devstack-down.cmd`, `devstack-restart.cmd`.
- [ ] Remove devstack PowerShell helpers (`devstack-*.ps1`) after migration.
- [ ] Update troubleshooting docs to reference Aspire instead of scripts.

## Open Questions

None.

Document decisions here as the concept evolves.
