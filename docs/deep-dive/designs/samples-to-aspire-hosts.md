# Samples to Aspire Hosts

::: info Status
Draft.
:::

## Intent

- Replace Croniq.Sample.\* hosts with Aspire-hosted containers built from Croniq.ApiHost, Croniq.WorkerHost, Croniq.WebhooksHost, and Croniq.DbMigrator.
- Remove Croniq.Sample.Jobs for now; reintroduce runner samples once multi-language SDKs are available.

## Decisions

- Delete Croniq.Sample.ApiHost, Croniq.Sample.WorkerHost, and Croniq.Sample.Dmz.
- Replace Croniq.Sample.Dmz with a Croniq.WebhooksHost container configured for StoreOnly ingress and `WebhookAdminOnly` API mode.
- Keep the Aspire AppHost as the canonical dev/CI entrypoint; do not add new Compose-based devstack flows.
- Run Croniq.WebhooksHost ingress alongside the devstack by default; stop it in the Aspire dashboard if you want a lighter local stack.

## Recommendation

- Runner samples should be owned by the SDK for their language and updated alongside SDK releases.
- Keep one canonical runner per language with minimal jobs to reduce maintenance.
- Integrate runner samples into the AppHost via opt-in profiles so the default devstack stays fast.

## Runner samples

- Location: `samples/runners/<language>/<name>`.
- SDK ownership: each sample tracks its SDK language folder.
- Scope:
  - Use only public Croniq APIs.
- Prefer gRPC-first dispatch with HTTP polling fallback.
  - Require explicit tenantId and environment tag; no default tenant fallback.
  - Read configuration from `Croniq__*` environment variables.
- Devstack integration: runner samples are opt-in and added via AppHost profiles.
- Documentation: each runner sample ships its own README with setup, expected outputs, and verification steps.

## References

- `../architecture.md`
- `../devstack.md`
- `polyglot-runner-protocol.md`
