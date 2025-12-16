# Croniq.Ui

Angular 21 (standalone) admin UI for Croniq. The app is configured to run **zoneless** and uses Tailwind for styling.

## Prerequisites

- Node.js (use the repo/toolchain standard; `packageManager` is pinned in `package.json`)
- `npm install`

## Development

- Start dev server: `npm start` (default: http://localhost:4200)
- Build: `npm run build`

### Runtime config

Runtime config is loaded from `public/assets/croniq-config.json` (optional). Supported keys:

- `apiBaseUrl`: absolute URL (`http/https`) or absolute path (`/...`)
- `swaggerUiUrl`: optional override (absolute URL or absolute path)

See `src/app/core/api-config.ts` and `src/app/core/runtime-config.service.ts`.

### Tests (watch vs. once)

Angular 21 uses the `@angular/build:unit-test` builder (Vitest). In interactive terminals, `ng test` defaults to watch mode.

- Watch mode (local dev): `npm test`
- Run once (recommended for CI / quick verification): `npm run test:once`
- CI mode (explicit `CI=1` + run once): `npm run test:ci`
- List tests: `npm run test:list`

Note: `npm run test -- --watch=false` may still enter watch mode in some shells/TTY setups; prefer `--no-watch` via `test:once`.

### Zoneless

Zoneless change detection is enabled in `src/app/app.config.ts` via `provideZonelessChangeDetection()`.

Practical implications:

- Prefer Signals (`signal`, `computed`, `effect`) for UI state.
- If you update non-signal state from async callbacks, you may need `ChangeDetectorRef.markForCheck()`.

## OpenAPI  Zod generation

Generate runtime-safe Zod schemas and endpoint definitions from the upstream OpenAPI document:

- Offline-friendly (prefers snapshot): `npm run generate:api`
- Force live swagger (local devstack): `npm run generate:api:server`

Input resolution order for `generate:api`:

1. `CRONIQ_OPENAPI_URL` (when set)
2. `artifacts/swagger.json` (snapshot)
3. `http://localhost:5000/swagger/v1/swagger.json` (fallback)

Generated output:

- `projects/api-schema/generated/` (overwritten)
- Templates: `tools/templates/`

Details: `docs/deep-dive/api-schema.md`.

## Angular MCP server

Dev-only helper to enable workspace-aware Angular operations via MCP:

- Start: `npm run mcp`
- VS Code task: `Angular MCP Server`

