# CHECKLIST-WEBHOOKS (UI)

_Last updated: 2026-01-18_

## Implementation notes

- Use `cq-data-grid` for the endpoints list.
- Implement row actions via a new context menu component.
- Build search + filter controls with Signal Forms.
- Organize new form primitives in `src/Croniq.Ui/projects/ui-kit`.

## Phase 1: Management baseline

- [ ] Endpoints list using `cq-data-grid` with columns (hook key, job key, status, signature mode, RPM, IP rules, last delivery).
- [ ] Search + filters (hook key, job key, status, environment) implemented via Signal Forms, with pagination and empty/error states.
- [ ] Context menu for row actions (edit, rotate secret, IP rules, delete/disable) with confirmations.
- [ ] Endpoint detail view showing effective configuration and derived ingress URL.
- [ ] Create/edit dialog with validation and inline help for hook key, job key, RPM, and signatures (ui-kit form primitives).
- [ ] Permission states for `webhooks:read` and `webhooks:write` (blocked view + CTA).

## UI Kit additions (Phase 1)

- [ ] Form primitives for text input, select, textarea, toggle, hint, and error states.
- [ ] Form field wrapper layout for label + description + error placement.
- [ ] Context menu component for row actions (keyboard + pointer).

## Phase 2: Security & hygiene

- [ ] Secret rotation flow (activate/grace windows, notes, one-time secret display).
- [ ] IP allow list management (list/create/delete, CIDR validation, bulk import).
- [ ] Signature policy UX driven by capabilities (allow unsigned only when permitted).

## Phase 3: Diagnostics & recovery

- [ ] Dead-letter list with filters, detail view, and replay actions.
- [ ] Delivery event timeline per endpoint (status, reason, timestamps, correlation ID).
- [ ] Action log panel showing recent admin operations from the UI.

## Phase 4: Testing & operator tooling

- [ ] Manual invoke/test payload panel with request preview and safe defaults.
- [ ] Copyable cURL/snippet examples for the configured endpoint.
- [ ] Bulk enable/disable endpoints with confirmation and audit context.

## Phase 5: Observability & insights

- [ ] Webhook KPIs (success rate, latency, rate-limit rejections) and trend tiles.
- [ ] Grafana deep-links or embedded panels where available.
- [ ] Audit summary for rotations, IP rule changes, and failed deliveries.

## Backend dependencies (by phase)

- [ ] Phase 1: `GET /tenants/{tenantId}/webhooks`, `POST /tenants/{tenantId}/webhooks`, `DELETE /tenants/{tenantId}/webhooks/{hookKey}`, `GET /tenants/{tenantId}/webhooks/capabilities`; list responses should expose `status`, `lastDeliveryAtUtc`, and `ipRules` or `ipRuleCount`.
- [ ] Phase 2: `POST /tenants/{tenantId}/webhooks/{hookKey}/rotate-secret`, `GET/POST/DELETE /tenants/{tenantId}/webhooks/{hookKey}/ip-rules`, plus `allowUnsignedHooks` in capabilities.
- [ ] Phase 3: `GET /tenants/{tenantId}/webhooks/deadletters`, `POST /tenants/{tenantId}/webhooks/deadletters/{deadLetterId}/replay`, `POST /tenants/{tenantId}/webhooks/deadletters/{deadLetterId}:resolve` (and optional `:fail`), plus an endpoint events feed for per-hook timelines.
- [ ] Phase 4: `POST /tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}` for manual invoke, published in OpenAPI.
- [ ] Phase 5: Telemetry-backed aggregates for webhook KPIs (Grafana URL or a dedicated API surface).
