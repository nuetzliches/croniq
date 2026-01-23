# Croniq UI

This document describes the current Croniq UI implementation (Angular 21 + Tailwind) and the near-term backlog.

## Current Stack

- Angular 21 standalone application using the `@angular/build` (Vite-powered) builders
- **Zoneless** change detection enabled via `provideZonelessChangeDetection()`
- Styling via Tailwind (see `tailwind.config.js`, `src/styles.css`)
- Libraries under `projects/`:
  - `projects/data-access`: API access helpers
  - `projects/api-schema`: generated Zod schemas + endpoint definitions
  - `projects/telemetry`: telemetry helpers
  - `projects/ui-kit`: UI primitives

## Language Policy

- The UI language is **English only**.
- This includes all user-facing copy in templates, UI-kit components, command palette labels, and accessibility text (`aria-*`, `sr-only`).
- Avoid introducing German strings in the Angular application; keep internal identifiers and API field names unchanged.

Auth notes and guardrails live in `docs/deep-dive/auth.md`.

## Repository Layout (current)

```
src/
	app/
	main.ts
	styles.css
projects/
	api-schema/
	data-access/
	telemetry/
	ui-kit/
public/
	assets/
		croniq-config.json
```

## Runtime Configuration

The UI loads an optional runtime config from `public/assets/croniq-config.json`.

- Schema + validation: `src/app/core/api-config.ts` (Zod)
- Loader: `src/app/core/runtime-config.service.ts`

Supported keys:

- `apiBaseUrl` (absolute URL or absolute path)
- `swaggerUiUrl` (optional override)
- `grafanaUrl` (optional absolute URL for the Grafana embed)
- `defaultTenantId` (optional; pre-fills tenant ID on the login screen and hides the tenant input)
- `webhooks.activityStream.mode` (`grpc`, `sse`, or `polling`)
- `webhooks.activityStream.grpcBaseUrl` (optional absolute URL or path for gRPC-Web proxy; defaults to `apiBaseUrl`)
- `webhooks.activityStream.sseBaseUrl` (optional absolute URL or path for SSE endpoint; defaults to `apiBaseUrl`)

Generate `public/assets/croniq-config.json` via `npm run generate:runtime-config` (runs automatically for `npm start`, `npm run build`, and `npm run watch`).
If `CRONIQ_UI_API_BASEURL` is not set, `CRONIQ_UI_API_PORT` plus optional `CRONIQ_UI_API_HOST` / `CRONIQ_UI_API_SCHEME` are used.
Set `CRONIQ_UI_DEFAULT_TENANT_ID` to emit `defaultTenantId` for single-tenant deployments.

Webhook capability flags (for example, unsigned webhook support) are resolved from the API capabilities endpoint and are not read from runtime config.

## Hosting & Container Image

Croniq.Ui ships as static assets hosted by `Croniq.UiHost`, the ASP.NET Core wrapper used for the `croniq-ui` container image. The host exposes `/health` and serves the Angular bundle plus a dynamic `assets/croniq-config.json` response.

- The response starts from the on-disk `wwwroot/assets/croniq-config.json` (use this for `grafanaUrl` defaults) and applies environment overrides.
- Supported overrides: `CRONIQ_UI_API_BASEURL` (preferred) or `CRONIQ_UI_API_PORT` plus optional `CRONIQ_UI_API_HOST`/`CRONIQ_UI_API_SCHEME`, `CRONIQ_UI_SWAGGER_UI_URL` (or `CRONIQ_UI_SWAGGER_URL`), `CRONIQ_UI_DEFAULT_TENANT_ID`, `CRONIQ_UI_WEBHOOKS_ACTIVITY_STREAM_MODE`, `CRONIQ_UI_WEBHOOKS_ACTIVITY_GRPC_BASEURL`, `CRONIQ_UI_WEBHOOKS_ACTIVITY_SSE_BASEURL`.
- The API base URL must be reachable from the browser (avoid internal container hostnames).

Small deployments can still serve the static assets behind `Croniq.Api`, but the default production path is the `croniq-ui` container so health checks and observability align with other services.

## Tenancy (Single Tenant)

Croniq.Ui currently targets single-tenant deployments. The UI does not expose tenant management routes or command palette entries. API Access remains available for managing tenant API keys/clients within the active tenant. Multi-tenant UI is deferred to vNext.

## Design Tokens & Motion

Design tokens (ramps, typography, motion durations) and layout primitives are implemented in `src/styles.css` and `tailwind.config.js`. The current token catalog and guidance live in `docs/deep-dive/designs/angular-ui-theme.md`.

## Icons

Croniq.Ui bundles a curated Material Design Icons (MDI) subset locally (no runtime icon fetch).

- Component: `cq-icon` from `projects/ui-kit/src/lib/icon/icon.ts`.
- Registry: `projects/ui-kit/src/lib/icon/mdi-icons.ts`.
- Names follow the Iconify MDI set (`https://icon-sets.iconify.design/mdi/`).
- Default registry includes: `magnify`, `refresh`, `plus`, `pencil`, `trash-can-outline`, `chevron-left`, `chevron-right`, `chevron-up`, `chevron-down`, `close`, `check`, `alert-outline`, `information-outline`, `content-copy`, `filter-remove`.
- Extend the registry by copying the icon body from `https://api.iconify.design/mdi.json?icons=<name>` and adding it to `mdi-icons.ts`.
- Size defaults to `1em` and inherits font-size; omit `size` to scale with surrounding text, or set an explicit value.
- Usage:
  ```html
  <cq-icon name="magnify" ariaLabel="Search" />
  ```

## Preferences

- UI preferences (theme, table density) are stored per tenant in IndexedDB via `UiPreferencesService`.
- Storage supports an optional encryption hook (`UI_PREFERENCES_CIPHER`) to wrap the serialized payload.

## API Schema & Generation

The UI uses runtime-safe Zod models generated from the upstream OpenAPI contract.

- Command (offline-friendly): `npm run generate:api`
- Command (force local swagger): `npm run generate:api:server`
- Output: `projects/api-schema/generated/` (overwritten)
- Templates: `tools/templates/`

Details: `docs/deep-dive/api-schema.md`.

## Tests

Unit tests use the Angular unit-test builder (Vitest).

- Watch mode: `npm test`
- Run once: `npm run test:once`

## Zoneless Notes

Zoneless is enabled in `src/app/app.config.ts`.

- Prefer Signals for UI state.
- For async updates that don't touch signals, use `ChangeDetectorRef.markForCheck()` where needed.

## Time & Dates

The UI standardizes timestamp handling via `src/app/core/time/clock.ts`.

- Prefer `nowMs()` / `nowIso()` for "current time".
- Prefer `isoFromEpochMs(epochMs)` for ISO formatting.
- Prefer `epochMsFromIso(iso)` for parsing ISO-ish strings.
- When reading date-ish values from `unknown` payloads (manual parsing, permissive API responses), normalize via `tryIsoFromUnknown(value)`.
- Avoid direct `new Date(...)`, `Date.now()`, `Date.parse(...)`, or `toISOString()` outside `clock.ts`.

## Live Dashboard Data

- Dashboard polling is the default; caching is disabled across UI resources (tenantRxResource cache hook is unused) and will be revisited after perf work.
- Streaming concepts (SSE-first) live in `docs/deep-dive/designs/dashboard-live-data.md`.

## MCP (dev-only)

The Angular MCP server is a development helper for workspace-aware automation.

- Start: `npm run mcp`
- VS Code task: `Angular MCP Server`

## Backlog

### Target Personas & Use Cases

1. Platform operators: monitor scheduler health, manage tenants and API keys, investigate failures.
2. Developers/job authors: browse job metadata, trigger manual executions, inspect logs/metrics.
3. Support/SRE: incident investigation, dead-letter browsing/replay, policy/limit verification.

### Delivery Phases

1. Scaffolding & Auth: keep the UI optional; align with backend auth readiness.
2. MVP data surfaces: dashboard (stubbed), schedules read-only, job registry.
3. Admin controls: CRUD for schedules/webhooks/API keys; dead-letter wiring.
4. Observability & polish: embed metrics/log views; accessibility review.

### Webhooks UI Roadmap

**Phase 1: Management baseline**

- Endpoint list view with columns for hook key, job key, status, signature mode, RPM, IP rules, and last delivery.
- Search + filters (hook key, job key, status, environment) with pagination and empty/error states.
- Row actions for edit, rotate secret, IP rules, and delete/disable with confirmations.
- Endpoint detail view (drawer or route) showing effective configuration and derived ingress URL.
- Create/edit dialog with validation and inline help for hook key, job key, RPM, and signatures.
- Permission states for `webhooks:read` and `webhooks:write` (blocked view + CTA).

**Phase 2: Security & hygiene**

- Secret rotation flow (activate/grace windows, operator notes) with one-time secret display + copy guardrails.
- IP allow list management per endpoint (list/create/delete, CIDR validation, bulk import).
- Signature policy UX driven by capabilities (allow unsigned only when permitted).

**Phase 3: Diagnostics & recovery**

- Dead-letter list with filters, detail view, and replay actions.
- Delivery event timeline per endpoint (success/failure, reason, timestamps, correlation ID).
- Action log panel showing recent admin operations from the UI.

**Phase 4: Testing & operator tooling**

- Manual invoke/test payload panel with request preview and safe defaults.
- Copyable cURL/snippet examples for the configured endpoint.
- Bulk enable/disable endpoints with confirmation and audit context.
- Remote delivery status (show base URL + `/health` probe when `Croniq:Webhooks:Mode=Remote`).

**Phase 5: Observability & insights**

- Webhook KPIs (success rate, latency, rate-limit rejections) and trend tiles.
- Grafana deep-links or embedded panels where available.
- Audit summary for rotations, IP rule changes, and failed deliveries.

**Backend dependencies (by phase)**

- **Phase 1** `GET /tenants/{tenantId}/webhooks`, `POST /tenants/{tenantId}/webhooks`, `DELETE /tenants/{tenantId}/webhooks/{hookKey}`, `GET /tenants/{tenantId}/webhooks/capabilities`; list responses should expose `status`, `lastDeliveryAtUtc`, and `ipRules` or `ipRuleCount` for UI columns.
- **Phase 2** `POST /tenants/{tenantId}/webhooks/{hookKey}/rotate-secret`, `GET/POST/DELETE /tenants/{tenantId}/webhooks/{hookKey}/ip-rules`, plus `allowUnsignedHooks` in capabilities.
- **Phase 3** `GET /tenants/{tenantId}/webhooks/deadletters`, `POST /tenants/{tenantId}/webhooks/deadletters/{deadLetterId}/replay`, `POST /tenants/{tenantId}/webhooks/deadletters/{deadLetterId}:resolve` (and optional `:fail`), plus an endpoint events feed for per-hook timelines.
- **Phase 4** `POST /tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}` for manual invoke, published in OpenAPI.
- **Phase 5** Telemetry-backed aggregates for webhook KPIs (Grafana URL or a dedicated API surface).
