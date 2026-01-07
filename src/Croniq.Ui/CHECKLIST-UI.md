# Croniq UI Checklist

TEMPORARY FILE: This checklist is a short-lived status tracker.

Long-term documentation lives in:

- `README.md` (dev commands: start/tests/zoneless/OpenAPI generation/MCP)
- `docs/deep-dive/ui.md` (current architecture + backlog)
- `docs/deep-dive/api-schema.md` (OpenAPI → Zod generation)
- `docs/deep-dive/AUTH.md` (auth notes)

Derived from [docs/deep-dive/designs/angular-ui-concept.md](docs/deep-dive/designs/angular-ui-concept.md).

## Delivery Phases

- [x] Design Spike – produce wireframes and refreshed design tokens aligned with the concept doc. _(Wireframes captured in [docs/deep-dive/designs/angular-ui-wireframes.md](docs/deep-dive/designs/angular-ui-wireframes.md); token inventory drafted, awaiting stakeholder review.)_
- [x] Design Spike – finalize Tailwind theme plus typography approvals with stakeholders. _(Theme + typographic tokens documented in [docs/deep-dive/designs/angular-ui-theme.md](docs/deep-dive/designs/angular-ui-theme.md); awaiting stakeholder sign-off recorded there.)_
- [ ] Scaffolding & Auth – keep this phase as a tracker; implementation details are documented in `README.md` and `docs/deep-dive/AUTH.md`. _(Original plan: [docs/deep-dive/designs/angular-ui-scaffolding.md](docs/deep-dive/designs/angular-ui-scaffolding.md).)_
  - [x] Auth session persistence: store the opaque Croniq session token in `sessionStorage` only; never persist refresh data in `localStorage`/IndexedDB. _(Implemented via [src/app/core/auth/auth-session.service.ts](src/app/core/auth/auth-session.service.ts); forms wired in [src/app/core/tenant-context/tenant-context.html](src/app/core/tenant-context/tenant-context.html); rationale captured in [docs/deep-dive/AUTH.md](docs/deep-dive/AUTH.md).)_
  - [x] External login expansion: leave hooks for a PKCE-based login bootstrap (deferred) so we can swap session auth for standards-based login once backend is ready.
  - [x] Operator impersonation vs. delegated auth: document the interim plan (manual tenant/operator context vs. delegated auth) and revisit once backend signals GA for full delegated auth. _(Plan recorded in [docs/deep-dive/AUTH.md](docs/deep-dive/AUTH.md).)_
- [x] MVP Data Surfaces – deliver the dashboard metrics (stubbed), schedules read-only grid, and job registry view.
- [ ] Admin Controls – implement CRUD for schedules, webhooks, and API keys, including dead-letter replay wiring.
  - [x] Schedules (CRUD + Dead Letter Replay)
  - [x] Webhooks (CRUD)
  - [ ] API Access (CRUD)
    - [x] List View + Revoke Action (Store wired)
    - [x] Create Dialog (Component implemented)
    - [x] Handle creation result (show secret to user)
    - [x] Issue Key mechanism (secondary action)
- [ ] Observability & Polish – embed Grafana/log pulse views and complete the accessibility plus localization review.

## Guardrails & Dependencies

- [ ] Confirm backend prerequisites are complete (schedule/job/admin APIs, gRPC-Web proxy, finalized login story, observability feeds).
- [ ] Definition of ready (next slice): schedules CRUD + schedule dead-letters replay
  - [x] API endpoints exist in the OpenAPI contract and are generated into `projects/api-schema/generated`:
    - `GET /tenants/:tenantId/schedules?environment=...&jobKey?=...`
    - `GET /tenants/:tenantId/schedules/:triggerId?environment=...`
    - `DELETE /tenants/:tenantId/schedules/:triggerId?environment=...`
    - `POST /tenants/:tenantId/schedules` (Upsert; see semantics below)
    - `GET /tenants/:tenantId/schedules/deadletters?environment=...`
    - Replay endpoint for schedule dead-letters (exact route TBD by contract; must be present before UI wiring)
  - [x] `environment` is supported consistently for schedules + dead-letters (query param).
  - [x] Upsert semantics documented for UI wiring:
    - `POST /tenants/{tenantId}/schedules` is Upsert.
    - `triggerId` in body is optional; if missing it defaults to `{jobKey}:{cronExpression}`.
    - If `triggerId` is provided, create/update is keyed by this id.
    - Important: if `cronExpression` changes and UI omits `triggerId`, the server will create a new trigger.
  - [x] OpenAPI responses are modeled (avoid `z.void()` for critical flows): schedules list/detail, upsert result (at least persisted `triggerId`), dead-letters list, replay result.
  - [ ] Error behavior is stable and documented: `400` validation, `401/403`, `404`, and idempotency expectations for replay.
- [ ] Document hosting decision (static assets behind Croniq.Api vs. dedicated `Croniq.Ui` container) and readiness/liveness expectations.
- [ ] Validate new npm dependencies meet the MIT/Apache/BSD policy and record any exceptions before merge.
- [x] Publish the ARIA playbook (based on https://angular.dev/guide/aria/overview) in `docs/ai/aria.md` and reference it from the PR template so every feature answers for roles, focus order, and keyboard shortcuts.

## Repository & Tooling Setup

- [x] Scaffold `src/Croniq.Ui` structure (application + libraries `data-access`, `telemetry`, `ui-kit`, plus generated `api-schema`) as outlined in the concept doc.
- [x] Configure Tailwind per [https://next.angular.dev/guide/tailwind](https://next.angular.dev/guide/tailwind) and emit Croniq tokens via CSS variables (`--cq-*`).
- [x] Capture MCP server usage in `.vscode/tasks.json` and `docs/deep-dive/designs/angular-ui-concept.md`, including `npm run mcp` instructions. _(Script + VS Code task wired up; see concept/scaffolding docs for details.)_
- [x] Establish built-in Angular resource (`rxResource`) + signals boilerplate shared across feature modules. _(Core helper in [src/app/core/resource/tenant-rx-resource.ts](src/app/core/resource/tenant-rx-resource.ts); first usage in schedules store.)_

## Application Architecture

- [x] Implement the shell layout (command rail, tenant selector, status beacons, command palette). _(Tailwind-first shell + command rail now live in [src/app/shell/shell/shell.html](src/app/shell/shell/shell.html) with shared status beacons in [src/app/shared/status-beacon/status-beacon.ts](src/app/shared/status-beacon/status-beacon.ts).)_
- [x] Command palette: extract a headless controller (signals + keyboard orchestration) with ARIA-compliant wrappers so we can skin it via Tailwind without duplicating logic. _(Headless controller + utilities live in [src/app/shared/command-palette/command-palette.controller.ts](src/app/shared/command-palette/command-palette.controller.ts) and the Tailwind template in [src/app/shared/command-palette/command-palette.html](src/app/shared/command-palette/command-palette.html).)_
- [x] Implement split-pane layout pattern (summary cards + tabbed detail panes) per page using Angular Aria tabs (no shared page-sized component).
- [ ] Deliver feature modules:
  - [ ] Dashboard - queue depth spark lines, upcoming triggers list, misfire heat map.
  - [x] Schedules - list/detail views with JSON diff preview for policy delta inspection.
  - [x] Jobs - registry browser, manual trigger action, last-N execution view.
  - [x] Webhooks - ingress status, secret rotation UI, IP allow-list grid.
  - [ ] Runners - availability read-model (available runners list + heartbeat status).
  - [ ] Tenants & API keys - intentionally excluded (single-tenant UI); no menu/command entries. Tenant reference is still required for tenant-scoped API routes.

## Data Access & State

- [x] Generate REST clients directly from the upstream OpenAPI contract (or gRPC-Web bridge) and wrap them in the shared `ApiClient` service that injects telemetry headers. _(Implemented via `openapi-zod-client` generation to `projects/api-schema/generated`; scripts live in `package.json`.)_
- [x] Document & standardize OpenAPI source selection (snapshot vs. live server), including the recommended local dev commands and the fallback order. _(See `artifacts/README.md`; use `npm run generate:api` (snapshot/offline) or `npm run generate:api:server` (live).)_
- [x] Add a one-shot Swagger snapshot command (`npm run snapshot:swagger`) and a combined refresh command (`npm run generate:api:server:snapshot`) so the repo snapshot can be updated deterministically.
- [ ] Decide CI policy for OpenAPI sync: keep committing `artifacts/swagger.json` snapshots vs. generating from a live/staging swagger endpoint (and how to avoid flaky builds when the endpoint is unavailable).
- [ ] Wire newly generated endpoints into the relevant feature stores (no new UX):
  - [ ] tenants list/create/deactivate (deferred: tenant feature excluded in single-tenant UI)
  - [x] tenant api-clients list/upsert/delete
  - [x] executions list
  - [x] jobs list (registry)
  - [x] jobs get/delete
  - [x] schedule get/delete
  - [x] schedules upsert (POST Upsert) + schedule dead-letters list/replay
  - [x] token issuance endpoints
- [ ] Configure Angular Query caches, refetch policies, and tenant/env scoping helpers.
- [ ] Persist non-sensitive preferences (theme, table density) per tenant using IndexedDB with optional encryption.

## Styling & Design Language

- [ ] Finalize typography pairing (`Space Grotesk`, `IBM Plex Mono`) and encode in Tailwind theme.
- [ ] Define semantic color ramps (surface, accent, danger) for both light/dark ops themes.
- [ ] Specify motion patterns (panel sweep, counter flip) and implement reusable animation utilities.
- [ ] Create layout primitives (`stack`, `cluster`, density controls) to keep spacing on the 8px grid.
- [ ] Document the Tailwind enrichment plan (utility namespaces, component recipes, palette integration) and ensure headless components expose the hooks needed for utility-first theming.

## Security & Auth

- [ ] Implement PKCE-based interactive login plus fallback for short-lived API tokens behind VPN.
- [ ] Ensure secrets/tokens remain memory-only and never persist to local storage or IndexedDB.
- [ ] Surface `ICallerContext` metadata in the UI so operators see "acting as" context during manual actions.
- [ ] Respect backend-enforced feature flags; hide toggles unless the API advertises support.
- [x] Wire `/auth/login` username/password flow and parse the backend response defensively (incl. `expiresIn`) with unit tests. _(Implemented in [src/app/core/auth/password-auth.service.ts](src/app/core/auth/password-auth.service.ts) with coverage in [src/app/core/auth/password-auth.service.spec.ts](src/app/core/auth/password-auth.service.spec.ts).)_
- [x] Remove manual token overrides and token issuance from the tenant-context panel (UI cleanup for password login).
- [x] Confirm the backend's intended auth scheme (Bearer session token vs. `X-Croniq-Key`) is reflected in the upstream OpenAPI contract and align the UI accordingly. _(Swagger snapshot normalization + Bearer-only `/auth/change-password` are in place; UI uses Bearer session token.)_
- [x] Respect `passwordChangeRequired` by forcing `/change-password` and exposing logout/password-change UX entry points (sidebar + command palette).
- [x] Wire silent refresh end-to-end (preemptive refresh + retry-on-401 once; refresh token remains memory-only; clear auth state and redirect to `/login` on refresh failure), incl. unit tests. _(See [src/app/core/auth/auth-refresh-coordinator.service.ts](src/app/core/auth/auth-refresh-coordinator.service.ts), [src/app/core/auth/auth-refresh.interceptor.ts](src/app/core/auth/auth-refresh.interceptor.ts), and specs in [src/app/core/auth/auth-refresh-coordinator.service.spec.ts](src/app/core/auth/auth-refresh-coordinator.service.spec.ts) + [src/app/core/auth/auth-refresh.interceptor.spec.ts](src/app/core/auth/auth-refresh.interceptor.spec.ts).)_

## Tooling, AI & Automation

- [x] Document workflow for Angular MCP server and ensure it aligns with [https://next.angular.dev/ai/develop-with-ai](https://next.angular.dev/ai/develop-with-ai). _(Concept + scaffolding docs now reference the Angular best-practices context and MCP launch steps.)_
- [ ] Apply the Angular AI design-patterns checklist [https://next.angular.dev/ai/design-patterns](https://next.angular.dev/ai/design-patterns) when reviewing generated code.
- [ ] Keep Storybook/Vitest/Playwright scripts wired into CI (npm run lint/test/build) and publish instructions in CONTRIBUTING.

## Build, Test & Release

- [ ] Wire `npm run build`, `npm run lint`, and `npm run lint:styles` into CI gates, mirroring backend pipelines.
- [ ] Resolve the current Angular build budget warning (initial bundle exceeds the configured budget); decide whether to optimize bundle size or update budgets with justification.
- [ ] Maintain Vitest unit coverage thresholds and Playwright E2E smoke tests against the devstack.
- [ ] Configure Storybook (Chromatic optional) for visual regression coverage on shared components.
- [ ] Publish build artifacts to `eng/artifacts/ui` and containerize the UI for deployment parity with other Croniq services.

### Developer instructions

This checklist intentionally avoids duplicating how-tos.

- Tests (watch vs. once): see `README.md`
- Zoneless notes: see `README.md` and `docs/deep-dive/ui.md`
- Time & dates policy: see `docs/deep-dive/ui.md` ("Time & Dates")

## Open Questions & Decisions

- [ ] Decide whether Grafana panels render inline (iframe) or via deep links and document CSP implications.
- [ ] Choose hosting domain strategy (shared with Croniq.Api vs. `ui.croniq.dev`) for cookie reuse and CSP constraints.
- [ ] Determine timeline for tenant impersonation features before GA.
- [ ] Define prefetch strategy (hover-driven vs. manual fetch) balancing responsiveness and API load; document final call in the concept doc.

# Next Steps (2025-12-19)

- When the backend contract changes: run `npm run snapshot:swagger`, then `npm run generate:api`.
- Keep OpenAPI responses in sync with the backend (ideally add response schemas for `/auth/*` upstream so generation no longer yields `z.void()` responses).
- Keep `tenantId` / `environmentTag` unset in the login payload (server-configured defaults).
- For tenant-scoped API routes (`/tenants/:tenantId/*`), pass the **tenant reference** (see root docs: `docs/deep-dive/password-auth.md` and `AGENTS.md`).
- Decide CI policy for OpenAPI sync (snapshot commit vs. live/staging generation) and implement it.
- Establish Angular Query + Signals boilerplate shared across feature modules.

# Prüfen / Nachbessern

- [x] Zod-Modelle/Generator-Ausgabe geprüft; `passthrough()` ist in Zod v4 nicht deprecated. Kurzer Leitfaden in `docs/ai/zod.instructions.md`.
- [x] OpenAPI-Codegen validiert: `npm run generate:api` erfolgreich (2025-12-20).
- [x] Testsuite validiert: `npm run test:once` erfolgreich (2025-12-20).
