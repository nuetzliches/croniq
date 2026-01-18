# Croniq UI

Croniq.Ui is the optional Angular admin console for tenant-scoped management workflows (for example schedules, executions, webhooks, and auth). It is packaged separately from the API/worker hosts and talks to the same HTTP endpoints as other clients.

This VitePress site keeps UI internals out of the public documentation set. UI-specific design notes and implementation details live alongside the UI workspace and are intentionally not linked here to keep the docs build stable.

When documenting user-visible behavior (new fields, endpoints, workflows), update the relevant guides or deep-dive references in `docs/` rather than UI internals.
