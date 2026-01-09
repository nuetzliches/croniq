# Croniq.Ui

Angular 21 (standalone) admin UI for Croniq. The app is configured to run **zoneless** and uses Tailwind for styling.

## Prerequisites

- Node.js (use the repo/toolchain standard; `packageManager` is pinned in `package.json`)
- `npm install`

## Development

- Start dev server: `npm start` (default: http://localhost:5081)
- Build: `npm run build`

### Runtime config

Runtime config is loaded from `public/assets/croniq-config.json` (optional). Supported keys:

- `apiBaseUrl`: absolute URL (`http/https`) or absolute path (`/...`)
- `swaggerUiUrl`: optional override (absolute URL or absolute path)

Generate the file via `npm run generate:runtime-config` (runs automatically for `npm start`, `npm run build`, and `npm run watch`).
Environment variables: `CRONIQ_UI_API_BASEURL`, `CRONIQ_UI_SWAGGER_UI_URL`.
If `CRONIQ_UI_API_BASEURL` is not set, `CRONIQ_UI_API_PORT` plus optional `CRONIQ_UI_API_HOST` / `CRONIQ_UI_API_SCHEME` are used.

Webhook capability flags (e.g., unsigned webhook support) are loaded from the API capabilities endpoint instead of the runtime config file.

See `src/app/core/api-config.ts` and `src/app/core/runtime-config.service.ts`.

### Tests (watch vs. once)

Angular 21 uses the `@angular/build:unit-test` builder (Vitest). In interactive terminals, `ng test` defaults to watch mode.

- Watch mode (local dev): `npm test`
- Run once (recommended for CI / quick verification): `npm run test:once`
- CI mode (explicit `CI=1` + run once): `npm run test:ci`
- List tests: `npm run test:list`

Note: `npm run test -- --watch=false` may still enter watch mode in some shells/TTY setups; prefer `--no-watch` via `test:once`.

## Lint

- Run: `npm run lint`
- Note: generated build output under `out-tsc/` is ignored by ESLint to avoid parsing errors.

### Zoneless

Zoneless change detection is enabled in `src/app/app.config.ts` via `provideZonelessChangeDetection()`.

Practical implications:

## Auth (dev)

The UI supports a simple username/password login against the backend `/auth/*` routes.

- Open `http://localhost:5081/login`
- The access token is stored in `sessionStorage` only.
- Tenant/environment are server-configured and are not set in the login payload.

## OpenAPI → Zod generation

Generate runtime-safe Zod schemas and endpoint definitions from the upstream OpenAPI document:

- Offline-friendly (prefers snapshot): `npm run generate:api`
- Force live swagger (local devstack): `npm run generate:api:server`

Input resolution order for `generate:api`:

1. `CRONIQ_OPENAPI_URL` (when set)
2. `artifacts/swagger.json` (snapshot)
3. `http://localhost:5080/swagger/v1/swagger.json` (fallback)

Generated output:

- `projects/api-schema/generated/` (overwritten)
- Templates: `tools/templates/`

Details: `docs/deep-dive/api-schema.md`.

## Angular MCP server

Dev-only helper to enable workspace-aware Angular operations via MCP:

- Start: `npm run mcp`
- VS Code task: `Angular MCP Server`
