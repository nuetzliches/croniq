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

Auth notes and guardrails live in `docs/deep-dive/AUTH.md`.

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
- `webhooksAllowUnsignedHooks` (derived from `Croniq:Webhooks:Security:AllowUnsignedHooks` when the sample ApiHost settings are present; defaults to `false`)

Generate `public/assets/croniq-config.json` via `npm run generate:runtime-config` (runs automatically for `npm start`, `npm run build`, and `npm run watch`).
If `CRONIQ_UI_API_BASEURL` is not set, `CRONIQ_UI_API_PORT` plus optional `CRONIQ_UI_API_HOST` / `CRONIQ_UI_API_SCHEME` are used.

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
