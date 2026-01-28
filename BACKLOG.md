# BACKLOG

## Docs

- [ ] Add docs publishing workflow (GitHub Pages) once the repo is public to avoid private-repo costs.
- [ ] Expand gRPC docs for Python/Go/Node (optional Java) with packages, install, auth helpers, minimal examples (deferred to vNext).

## UI

- See [src/Croniq.Ui/BACKLOG.md](src/Croniq.Ui/BACKLOG.md).

## Calendars

- [ ] Add UI tests for calendar assignment and calendar views in Croniq.Ui.
- [ ] Add calendar definition caching per tenant/environment with CRUD invalidation and a cache-hit metric.
- [ ] Add optional `InheritTriggerTimeZone` to reuse the schedule time zone when needed.
- [ ] Support ordered multi-calendar assignments with explicit precedence.
- [ ] Make calendar guard limits configurable (or allow a wider lookahead mode).
- [ ] Introduce per-schedule override layers for temporary exclusions/inclusions.

## Platform

- [ ] (deferred – waits on explicit stakeholder request) Prepare Kubernetes chart placeholder (charts/croniq) per [docs/deep-dive/kubernetes.md](docs/deep-dive/kubernetes.md).
- [ ] (deferred – vNext) Publish non-.NET gRPC client packages (Python/PyPI, Go module, Node/NPM) and update samples to reference the packages.

## Runners

- [ ] Improve runner UI visibility: transport state, last seen, max inflight, allow-test flag, and drain status per runner instance.
- [ ] Persist runner presence beyond OnlineTtl and expose includeOffline (retention-based) to allow UI to show offline runners.
- [ ] Allow runner SDKs to upsert schedules (opt-in; requires schedules:write scope).
- [ ] Support once-per-deployment/cluster @once scheduling from runners (define deployment/cluster identity and deterministic triggerId to avoid re-arming races).
- [ ] Add adaptive polling guidance (server hints for next poll delay; keep SDK jittered backoff).
- [ ] Add optional job-key allowlists or capability routing to reduce unneeded work assignments.
- [ ] Emit queue health metrics (lease age, pending work count per job/environment) for scale tuning.

## Tooling

- [ ] Add dotnet templates: `dotnet new croniq-worker` / `dotnet new croniq-platform` with minimal `appsettings.json`.
- [ ] Optional CLI/Dev tool: trigger list, next runs, config validation, export/import (e.g., `dotnet tool`).
- [ ] CI check: fail if EF Core migrations are missing designer files or `dotnet ef migrations list` does not include the latest migration for SqlServer/Postgres.

## Infrastructure

- [ ] Harden defaults in [infra/docker/docker-compose.production.yml](infra/docker/docker-compose.production.yml) (no InMemory auth/smoke key, no admin/admin seeding, no `Encrypt=False`/`TrustServerCertificate=True`, `ExposeSchemas` false).
- [ ] Avoid `MSSQL_PID=Developer` default for production; enforce explicit value or document clearly.

## Core

- [ ] Address or document the performance TODO in `CronExpression.cs`.
- [ ] Add registry sync/remote lookup so ApiHost can trigger jobs without local registrations.

## Engineering Hygiene

- [ ] (nice-to-have) Clean up solution-wide usings.

## Security & Quality

- [ ] Decide on CI static analysis / SAST and integrate if needed.
  - [ ] Add CodeQL code-scanning workflow (optional; depends on GHAS/Repo settings).
  - [ ] Evaluate SonarQube (signal/noise, cost, gate policy).
  - [ ] Review Roslyn analyzers/ruleset (e.g., .editorconfig/Directory.Build.props) and enable only high-signal rules.

## Auth

- [x] Make OIDC optional + configurable (e.g., Authelia) while keeping the default password login fully supported by croniq-api without external providers.
- [x] Auth concept: replace password login with external login flow using PKCE (OIDC).
- [x] Auth concept: route access-token distribution through HttpInterceptors so feature modules can call `HttpClient` directly.
- [x] Auth concept: logout clears session storage and any relevant client caches.
- [ ] Auth concept: document CSP changes once the login redirect domain is finalized.
- [ ] Auth concept: define and document default claim mappings for Authelia (tenant/env/scope/callerId) with a minimal example.
- [ ] Auth concept: add audit logging (auth.AuditLog table + structured logs) for key issuance/rotation/revocation, token issue/failure, login failures, and password changes.
- [ ] Auth concept: add tenant lifecycle hooks for create/update/deactivate (audit events, quota metadata, default environment tags).
- [ ] Auth concept: add CLI automation scripts for issuing/rotating keys and minting tokens (PowerShell + optional Bash), documented in guides.
