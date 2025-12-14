# Croniq UI Checklist

Derived from [src/Croniq.Ui/docs/deep-dive/designs/angular-ui-concept.md](src/Croniq.Ui/docs/deep-dive/designs/angular-ui-concept.md). Update these tasks as the Angular 21 + Tailwind admin UI progresses.

## Delivery Phases

- [x] Design Spike – produce wireframes and refreshed design tokens aligned with the concept doc. _(Wireframes captured in [src/Croniq.Ui/docs/deep-dive/designs/angular-ui-wireframes.md](src/Croniq.Ui/docs/deep-dive/designs/angular-ui-wireframes.md); token inventory drafted, awaiting stakeholder review.)_
- [x] Design Spike – finalize Tailwind theme plus typography approvals with stakeholders. _(Theme + typographic tokens documented in [src/Croniq.Ui/docs/deep-dive/designs/angular-ui-theme.md](src/Croniq.Ui/docs/deep-dive/designs/angular-ui-theme.md); awaiting stakeholder sign-off recorded there.)_
- [ ] Scaffolding & Auth – initialize the Angular workspace in `src/Croniq.Ui`, configure MCP helper tasks, and wire the OIDC stub with the tenant switcher. _(Plan documented in [src/Croniq.Ui/docs/deep-dive/designs/angular-ui-scaffolding.md](src/Croniq.Ui/docs/deep-dive/designs/angular-ui-scaffolding.md); pending execution of `ng new`, library generation, Tailwind tokens, and OIDC stub.)_
  - [ ] Auth session persistence: store the opaque Croniq session token in `sessionStorage` only; never persist refresh data in `localStorage`/IndexedDB. _(Documented in [AUTH.md](src/Croniq.Ui/docs/deep-dive/AUTH.md), includes TODO to rotate keys during logout.)_
  - [ ] OAuth/OIDC expansion: leave hooks for PKCE/OIDC bootstrap (deferred) so we can swap session auth for standards-based login once backend is ready.
  - [ ] ApiKey bootstrap: surface an operator-facing form to capture the short-lived Croniq API key (sent as `X-Croniq-Key`) and inject it through `EndpointExecutor` for every call until OIDC replaces it.
  - [ ] Operator impersonation vs. OAuth: document the interim plan (manual tenant/operator context vs. delegated auth) and revisit once backend signals GA for full OAuth.
- [ ] MVP Data Surfaces – deliver the dashboard metrics (stubbed), schedules read-only grid, and job registry view.
- [ ] Admin Controls – implement CRUD for schedules, webhooks, and API keys, including dead-letter replay wiring.
- [ ] Observability & Polish – embed Grafana/log pulse views and complete the accessibility plus localization review.

## Guardrails & Dependencies

- [ ] Confirm backend prerequisites are complete (schedule/job/admin APIs, gRPC-Web proxy, finalized OIDC story, observability feeds).
- [ ] Document hosting decision (static assets behind Croniq.Api vs. dedicated `Croniq.Ui` container) and readiness/liveness expectations.
- [ ] Validate new npm dependencies meet the MIT/Apache/BSD policy and record any exceptions before merge.
- [ ] Publish the ARIA playbook (based on https://angular.dev/guide/aria/overview) in `docs/ai/aria.md` and reference it from the PR template so every feature answers for roles, focus order, and keyboard shortcuts.

## Repository & Tooling Setup

- [ ] Scaffold `src/Croniq.Ui` structure (apps/admin, libs/data-access, libs/telemetry, libs/ui-kit) as outlined in the concept doc.
- [ ] Configure Tailwind per [https://next.angular.dev/guide/tailwind](https://next.angular.dev/guide/tailwind) and emit Croniq tokens via CSS variables (`--cq-*`).
- [x] Capture MCP server usage in `.vscode/tasks.json` and `src/Croniq.Ui/docs/deep-dive/designs/angular-ui-concept.md`, including `npm run mcp` instructions. _(Script + VS Code task wired up; see concept/scaffolding docs for details.)_
- [ ] Establish Angular Query + Signals boilerplate shared across feature modules.

## Application Architecture

- [x] Implement the shell layout (command rail, tenant selector, status beacons, command palette). _(Tailwind-first shell + command rail now live in [src/app/shell/shell/shell.html](src/app/shell/shell/shell.html) with shared status beacons in [src/app/shared/status-beacon/status-beacon.ts](src/app/shared/status-beacon/status-beacon.ts).)_
- [x] Command palette: extract a headless controller (signals + keyboard orchestration) with ARIA-compliant wrappers so we can skin it via Tailwind without duplicating logic. _(Headless controller + utilities live in [src/app/shared/command-palette/command-palette.controller.ts](src/app/shared/command-palette/command-palette.controller.ts) and the Tailwind template in [src/app/shared/command-palette/command-palette.html](src/app/shared/command-palette/command-palette.html).)_
- [ ] Build split-pane content templates (summary cards + tabbed detail panes) reusable across modules.
- [ ] Deliver feature modules:
  - [ ] Dashboard – queue depth spark lines, upcoming triggers list, misfire heat map.
  - [ ] Schedules – list/detail views with JSON diff preview for policy delta inspection.
  - [ ] Jobs – registry browser, manual trigger action, last-N execution view.
  - [ ] Webhooks – ingress status, secret rotation UI, IP allow-list grid.
  - [ ] Tenants & API keys – quota management, key rotation, policy override visibility.

## Data Access & State

- [ ] Generate REST clients directly from the upstream OpenAPI contract (or gRPC-Web bridge) and wrap them in the shared `ApiClient` service that injects telemetry headers. _(Status: runtime-safe models now flow from the upstream spec via `npm run generate:api`, which runs `openapi-zod-client` and writes to `projects/api-schema/generated`. Manual helpers still live in `projects/api-schema/src`, but next we need to wire CI and evaluate client generation.)_
- [ ] Configure Angular Query caches, refetch policies, and tenant/env scoping helpers.
- [ ] Persist non-sensitive preferences (theme, table density) per tenant using IndexedDB with optional encryption.

## Styling & Design Language

- [ ] Finalize typography pairing (`Space Grotesk`, `IBM Plex Mono`) and encode in Tailwind theme.
- [ ] Define semantic color ramps (surface, accent, danger) for both light/dark ops themes.
- [ ] Specify motion patterns (panel sweep, counter flip) and implement reusable animation utilities.
- [ ] Create layout primitives (`stack`, `cluster`, density controls) to keep spacing on the 8px grid.
- [ ] Document the Tailwind enrichment plan (utility namespaces, component recipes, palette integration) and ensure headless components expose the hooks needed for utility-first theming.

## Security & Auth

- [ ] Implement OIDC PKCE login plus fallback for short-lived API tokens behind VPN.
- [ ] Ensure secrets/tokens remain memory-only and never persist to local storage or IndexedDB.
- [ ] Surface `ICallerContext` metadata in the UI so operators see "acting as" context during manual actions.
- [ ] Respect backend-enforced feature flags; hide toggles unless the API advertises support.
- [ ] Add Croniq API Key handling: map the captured token to the `X-Croniq-Key` header, validate expiry, and provide a single "Switch Operator" action that clears impersonation plus the key.

## Tooling, AI & Automation

- [x] Document workflow for Angular MCP server and ensure it aligns with [https://next.angular.dev/ai/develop-with-ai](https://next.angular.dev/ai/develop-with-ai). _(Concept + scaffolding docs now reference the Angular best-practices context and MCP launch steps.)_
- [ ] Apply the Angular AI design-patterns checklist [https://next.angular.dev/ai/design-patterns](https://next.angular.dev/ai/design-patterns) when reviewing generated code.
- [ ] Keep Storybook/Vitest/Playwright scripts wired into CI (npm run lint/test/build) and publish instructions in CONTRIBUTING.

## Build, Test & Release

- [ ] Wire `npm run build`, `npm run lint`, and `npm run lint:styles` into CI gates, mirroring backend pipelines.
- [ ] Maintain Vitest unit coverage thresholds and Playwright E2E smoke tests against the devstack.
- [ ] Configure Storybook (Chromatic optional) for visual regression coverage on shared components.
- [ ] Publish build artifacts to `eng/artifacts/ui` and containerize the UI for deployment parity with other Croniq services.

## Open Questions & Decisions

- [ ] Decide whether Grafana panels render inline (iframe) or via deep links and document CSP implications.
- [ ] Choose hosting domain strategy (shared with Croniq.Api vs. `ui.croniq.dev`) for cookie reuse and CSP constraints.
- [ ] Determine timeline for multi-tenant impersonation features before GA.
- [ ] Define prefetch strategy (hover-driven vs. manual fetch) balancing responsiveness and API load; document final call in the concept doc.
