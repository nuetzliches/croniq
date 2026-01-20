# Croniq UI

Croniq.Ui is the optional Angular admin console for tenant-scoped management workflows (for example schedules, executions, webhooks, and auth). It is packaged separately from the API/worker hosts and talks to the same HTTP endpoints as other clients.

This VitePress site keeps UI internals out of the public documentation set. UI-specific design notes and implementation details live alongside the UI workspace and are intentionally not linked here to keep the docs build stable.

## Webhook Activity Transport

Webhook activity streams prefer gRPC (via gRPC-Web/HTTP2 proxy), fall back to SSE when gRPC is unavailable, and finally fall back to polling. Until streaming is available, the UI uses polling with pause/retry controls.

When documenting user-visible behavior (new fields, endpoints, workflows), update the relevant guides or deep-dive references in `docs/` rather than UI internals.
