# CHECKLIST-SAMPLES-ASPIRATION

Checklist for deleting Croniq.Sample.* projects and moving Aspire to Docker images built from Croniq.*Host projects.

## Summary

Move the development/test entrypoint from Croniq.Sample.\* projects to the Aspire AppHost running Docker containers built from Croniq.ApiHost, Croniq.WorkerHost, and Croniq.WebhooksHost (plus Croniq.DbMigrator). This removes duplicated host configuration, aligns dev/CI with production hosts, and keeps a single execution path for API, worker, and ingress. There are no external consumers, so breaking changes are acceptable for this migration.

## Goals

- Maintain a single runtime path for all hosts (API, worker, webhooks/DMZ) in dev and CI.
- Remove duplicated hosting setup from Croniq.Sample.\* projects.
- Keep developer experience fast via the Aspire devstack (tools/Croniq.Devstack.AppHost).
- Remove sample jobs for now; reintroduce sample runners later once multi-language SDKs are in place.
- Use the opportunity to break and simplify host configuration where needed (no backward compatibility constraints).
- Keep OpenTelemetry-first observability intact across containers.
- Avoid new Compose-based devstack entrypoints.

## Non-goals

- Rewriting SDKs or client samples (beyond the minimum needed to remove sample hosts).
- Changing gRPC contracts or REST surface area.
- Changing persistence providers beyond what is required for containerization.
- Maintaining backward compatibility with Croniq.Sample.\* host projects.
- Introducing new host types beyond Croniq.ApiHost/Croniq.WorkerHost/Croniq.Webhooks unless explicitly required.

## Current state

- Croniq.Sample.\* projects host API and worker in-process with configuration that diverges from Croniq.ApiHost/Croniq.WorkerHost.
- Aspire/devstack currently depends on sample hosts for local execution.
- Shared hosting concerns (Kestrel protocols, gRPC endpoints, observability) are duplicated.
- `docs/deep-dive/devstack.md` and devstack scripts reference Croniq.Sample.\* hosts and their config defaults.

## Scope confirmation

- [x] Confirm that there are no external consumers and backward compatibility is not required for this change.
- [x] Croniq.Sample.\* projects will be deleted.
- [x] List all affected sample projects (ApiHost, WorkerHost, Dmz) and map to owning host image.
  - Croniq.Sample.ApiHost -> Croniq.ApiHost (container image built by Aspire).
  - Croniq.Sample.WorkerHost -> Croniq.WorkerHost (container image built by Aspire).
  - Croniq.Sample.Dmz -> Croniq.WebhooksHost (StoreOnly ingress) + Croniq.ApiHost (WebhookAdminOnly).
  - Croniq.Sample.Jobs -> remove now; reintroduce later as runner samples.
- [x] Decide whether samples/ remains for non-host assets (client samples, docs, scripts).
  - Keep samples/ for non-host assets (grpc-client-*, worker-sdk-*). Remove host projects from samples/.
- [x] Confirm no sample hosts remain; remove any references to deprecated wrappers.
- [x] Inventory docs/scripts that reference Croniq.Sample.\* and plan updates (devstack, guides, scripts).
  - [docs/deep-dive/devstack.md](docs/deep-dive/devstack.md#L26)
  - [docs/deep-dive/observability.md](docs/deep-dive/observability.md#L126)
  - [docs/deep-dive/testing.md](docs/deep-dive/testing.md#L19)
  - [docs/deep-dive/designs/dmz-ingress-remote-webhooks.md](docs/deep-dive/designs/dmz-ingress-remote-webhooks.md#L256)
  - [docs/deep-dive/designs/samples-to-aspire-hosts.md](docs/deep-dive/designs/samples-to-aspire-hosts.md#L9-L16)
  - [docs/introduction/deployment-modes.md](docs/introduction/deployment-modes.md#L33)
  - [docs/guides/webhooks.md](docs/guides/webhooks.md#L114)
  - [scripts/smoke-dmz.ps1](scripts/smoke-dmz.ps1#L270-L348)
  - [tools/Croniq.Devstack.AppHost/Program.cs](tools/Croniq.Devstack.AppHost/Program.cs#L411-L638)
  - [tools/Croniq.Devstack.AppHost/Croniq.Devstack.AppHost.csproj](tools/Croniq.Devstack.AppHost/Croniq.Devstack.AppHost.csproj#L16-L17)
  - [croniq.slnx](croniq.slnx#L8-L11)

## Proposed architecture

- [ ] Aspire becomes the default dev/test launcher (tools/Croniq.Devstack.AppHost).
  - Build Docker images from Croniq.ApiHost, Croniq.WorkerHost, Croniq.WebhooksHost, and Croniq.DbMigrator (reuse infra/docker Dockerfiles where possible).
  - Configure ports, environment variables, and secrets through the Aspire app model.
  - Keep optional UI, Caddy, and observability resources behind AppHost profiles.
- [ ] Shared hosting extensions.
  - Use existing `Croniq.Hosting`/`Croniq.Api`/`Croniq.Webhooks` extension points (`AddCroniqPlatformServices`, `AddCroniqWorkerServices`, `AddCroniqApiServices`, `UseCroniqApi`, `AddCroniqWebhookServices`, `UseCroniqWebhooks`).
  - Ensure both production hosts and any remaining sample tooling reuse the same setup without sample-only branches.
- [ ] DMZ ingress topology aligns with the architecture guide.
  - Replace Croniq.Sample.Dmz with a Croniq.WebhooksHost container running `Ingress:DispatchMode=StoreOnly` and `Croniq.Api` in `WebhookAdminOnly`.
  - Internal hosts use `Croniq:Webhooks:Mode=Remote` and run the relay in WorkerHost; avoid introducing a new Croniq.DmzHost unless requirements force it.
- [ ] Croniq.WebhooksHost runs by default in devstack; provide an opt-out flag/profile for lighter local runs.
- [ ] Sample project removal.
  - Delete Croniq.Sample.ApiHost, Croniq.Sample.WorkerHost, and Croniq.Sample.Dmz entirely.
- [ ] Sample content preservation.
  - Remove Croniq.Sample.Jobs for now.
  - Track a follow-up for per-language runner samples under samples/ once multi-language SDKs are ready.

## Configuration model

- [ ] Aspire injects environment variables for:
  - Standard .NET `Croniq__*` and `ConnectionStrings__*` bindings (AppHost-only controls remain `CRONIQ_DEVSTACK_*`).
  - `Croniq__Api__*`, `Croniq__WorkerDispatch__*`, `Croniq__Observability__*`, `Croniq__Webhooks__*`, and storage/provider settings.
- [ ] gRPC endpoints remain HTTP/2 capable (h2c for local dev, TLS for production).
- [ ] Webhook ingress/remote settings follow `Croniq:Webhooks:*` (including StoreOnly + Remote config for DMZ).
- [ ] Password auth still requires an explicit `tenantId` (no default-tenant fallback in sample/dev flows).
- [ ] API keys and secrets are injected via Aspire secret providers or development secrets (no inline secrets).

## Aspire integration

- [ ] Add Aspire app model changes to build Croniq.\*Host Docker images and Croniq.DbMigrator.
- [ ] Ensure Aspire app uses consistent ports and environment variables across hosts.
- [ ] Wire container dependencies (DbMigrator before API/Worker; Worker depends on API for gRPC dispatch).
- [ ] Add health checks for each container to surface failures in Aspire UI.
- [ ] Align gRPC endpoints with HTTP/2 (h2c or TLS) and document expected scheme.

## Host configuration alignment

- [ ] Extract shared hosting config into reusable extensions (Kestrel, gRPC mappings, observability, CORS) using the existing hosting packages.
- [ ] Ensure ApiHost/WorkerHost use the same defaults as current samples.
- [ ] Ensure dev configuration overlays are available via appsettings + env vars.

## Build and packaging

- [ ] Prefer `infra/docker/Dockerfile.services`/`infra/docker/Dockerfile.production` and shared base images over per-host Dockerfiles.
- [ ] Validate config injection paths (env vars, mounted secrets).
- [ ] Ensure images are compatible with CI and local Aspire devstack.

## Tests and validation

- [ ] Update smoke tests to target Aspire-managed containers (ApiHost/WorkerHost/WebhooksHost).
- [ ] Remove sample-host-specific test assumptions.
- [ ] Verify gRPC worker dispatch and fallback behavior in Aspire.
- [ ] Validate DMZ webhook relay flow (StoreOnly ingress + Remote relay).

## Documentation

- [ ] Update `docs/deep-dive/devstack.md` to describe the new Aspire-based host flow.
- [ ] Update docs to describe the new Aspire-based sample flow and host entrypoints.
- [ ] Remove any docs references to Croniq.Sample.\* (no deprecation window planned).

## Risks and mitigations

- [ ] Loss of quick in-process debugging.
  - Mitigation: support local non-container dotnet run on Croniq.\*Host with the same configs.
- [ ] Aspire build time increases.
  - Mitigation: use incremental Docker layers and optimize build context.
- [ ] Configuration drift.
  - Mitigation: centralize defaults in shared extensions and add tests.

## Open questions

- [x] Confirm the recommended ownership model for future multi-language runner samples under `samples/` (see docs/deep-dive/designs/samples-to-aspire-hosts.md).
