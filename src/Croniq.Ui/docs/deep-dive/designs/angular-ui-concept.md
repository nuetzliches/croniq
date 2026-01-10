# Croniq UI Concept (Angular 21 + Tailwind)

## Goals & Success Criteria

- Ship an opinionated admin UI that mirrors the personas and flows listed in `docs/deep-dive/ui.md` while staying optional for headless Croniq deployments.
- Keep the UI repo-local (no separate SPA repo) so architecture reviews and CI/CD remain aligned with backend changes.
- Deliver a design language that feels operational, dense, and telemetry-aware; dashboards must communicate queue state, policy health, and webhook posture without feeling like a generic CRUD generator.

## Guardrails & Dependencies

- Backend prerequisites: stable schedule/job/admin APIs, gRPC-Web proxy (if needed), finalized login story, and observability endpoints as called out in `docs/deep-dive/architecture.md` and `docs/deep-dive/ui.md`.
- Platform constraints: MIT/Apache/BSD-only dependencies, OpenTelemetry-first instrumentation, strict separation between secrets and UI bundles (no API keys in browser storage).
- Hosting: built artifacts deploy either as static assets behind Croniq.Api or via a slim `Croniq.Ui` container image that speaks the same readiness/liveness protocol as other services.

## Technology Decisions

| Concern            | Decision                                                        | Notes                                                                                                                                                                   |
| ------------------ | --------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Framework          | Angular 21 standalone apps, built with the Vite-powered builder | Gives SSR-ready hydration, Signals API, and CLI ergonomics familiar to enterprise contributors.                                                                         |
| Styling            | Tailwind CSS + custom tokens                                    | Tailwind provides utility primitives; we layer Croniq-specific tokens (semantic colors) via `tailwind.config.js` and CSS variables.                                     |
| Component strategy | Headless primitives + lightweight shims                         | Compose headless patterns (Angular Aria) with Tailwind classes; pages compose a two-pane layout (summary column + tabbed detail) without a shared page-sized component. |
| State/query        | Signals-first, typed services (optional query lib later)        | Current codebase is Signals-first + strict typing; if we add a query lib, it should be justified and used consistently (avoid half-migrations).                         |
| Forms              | Angular Signal Forms (experimental in v21) + Zod at the edges   | Prefer Signal Forms for new forms once we're comfortable with the API; use Zod to validate runtime config and API contracts.                                            |
| Testing            | Vitest                                                          | E2E/Storybook are optional future additions; don't document them as required until wired in `package.json`.                                                             |
| DX automation      | Angular MCP Server                                              | Dev-only helper (VS Code + MCP). Use `.vscode/mcp.json` + `npm run mcp` and keep it out of runtime builds.                                                              |

## Repository & Project Layout

```
angular.json
package.json
tailwind.config.js
tsconfig.json

src/
   app/
      core/
      shared/
      shell/
      features/
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

- Place the Angular workspace in `src/Croniq.Ui` so it lives next to other product code and inherits the same build/versioning pipelines.
- Use secondary entry points (libraries) to separate regulated surfaces (telemetry, auth) from visual components; this aides tree-shaking when we publish micro-frontends later.
- Tailwind uses CSS variables for semantic colors (see `src/styles.css` + `tailwind.config.js`); theme switching is applied via `data-theme` on `:root`.

## Application Architecture

1. **Shell & Layout**

   - Persistent command rail on the left with environment selector + status beacons (cluster health, clock skew, policy alerts).
   - Main canvas uses a page-local two-pane layout: summary column on the left and tabbed detail panel on the right (Angular Aria tabs) to emphasize live metrics.
   - Global command palette (Ctrl+K) surfaces job search, quick trigger, and navigation actions.

2. **Feature Modules**

   - `dashboard`: queue depth spark lines, upcoming triggers, misfire heat map (ECharts or visx).
   - `schedules`: list + detail editing surface with JSON diff viewer so operators can see policy deltas before saving.
   - `jobs`: registry browser, ability to trigger jobs manually, show last N executions.
   - `webhooks`: ingress status, secret rotation controls, IP allow-list grid.
   - `API keys`: manage quotas, rotate keys, view policy overrides.

3. **Data Access**

   - API contracts are generated from the upstream OpenAPI document into runtime-safe Zod schemas + endpoint definitions (see `docs/deep-dive/api-schema.md`).
   - The `projects/data-access` library owns request execution and auth/header injection.
   - Keep feature data access consistent: prefer one shared abstraction (executor/client) rather than ad-hoc `HttpClient` usage across features.

4. **State & Caching**
   - Use Signals for local ephemeral state (panel toggles, wizard steps) to avoid unnecessary RxJS complexity.
   - UI preferences (theme, density) are persisted per tenant in IndexedDB; see `docs/deep-dive/ui.md` for details.

## Styling & Design Language

- Typography: pair `Space Grotesk` (headings) with `IBM Plex Mono` (metrics) to create a console-inspired look.
- Color palette: deep charcoal backgrounds with saturated amber/cyan accents to highlight policy breaches or successful triggers.
- Motion: prefer intentful transitions (panel sweep, counter flip) over micro-animations. Global loading indicators appear as progress beams at the top edge.
- Layout primitives: Tailwind utility classes plus a `stack`/`cluster` set of CSS components to enforce spacing rhythm (8px grid).

## Security & Auth Integration

- Support interactive login (PKCE) for human operators; fallback to short-lived API tokens for service technicians behind VPN.
- Never persist secrets locally; rely on backend-managed session cookies or ephemeral tokens stored in memory.
- Integrate with `ICallerContext` metadata so the UI can show "acting as" context and log audit trails for manual actions.
- Enforce feature flags in the backend as well; the UI only exposes toggles that the API already guards.

## Tooling & MCP Server Usage

- The Angular MCP Server runs alongside VS Code so tools/agents can execute workspace-aware tasks (create components, update routes) while honoring repo guardrails.
- Launch the server locally with `npm run mcp` (or the "Angular MCP Server" VS Code task). `.vscode/mcp.json` wires VS Code to the Angular CLI MCP endpoint documented at https://angular.dev/ai/mcp.
- MCP stays dev-only: no runtime dependency, no shipped assets, and the `servers.angular-cli` entry only runs in local dev shells.
- Use the server to codify scaffolding recipes (e.g., `generate feature schedules --with-crud --with-grid`), ensuring consistent folder structure/tests.
- Align MCP prompts and automation scripts with Angular's AI guidance: leverage the "Develop with AI" workflows [https://next.angular.dev/ai/develop-with-ai](https://next.angular.dev/ai/develop-with-ai), include the official best-practices context file when prompting, and apply the recommended AI design patterns [https://next.angular.dev/ai/design-patterns](https://next.angular.dev/ai/design-patterns) to validate what the agent produces before committing it.

## Build, Test & Release Flow

1. **Build**: `npm run build` uses Angular's Vite builder; Tailwind compiled during build to minimize runtime CSS cost.
2. **Unit tests**: `npm test` (watch) / `npm run test:once` (single run).
3. **Packaging**: `npm run build` emits `dist/` artifacts (exact output path is defined by the Angular builder).

Optional future additions (only when implemented): Playwright E2E, Storybook.

## Delivery Phases

1. **Design Spike** (1 sprint)
   - Produce wireframes + updated design tokens.
   - Finalize Tailwind theme + typography approvals with stakeholders.
2. **Scaffolding & Auth** (1 sprint)
   - Initialize Angular workspace, configure MCP tasks, hook up login bootstrap stub + environment switcher.
3. **MVP Data Surfaces** (2 sprints)
   - Dashboard metrics (stubbed), schedules read-only grid, job registry view.
4. **Admin Controls** (2 sprints)
   - CRUD for schedules/webhooks/API keys; integrate dead-letter replay.
5. **Observability & Polish** (2 sprints)
   - Grafana embedding, log pulse view, accessibility + localization review.

## Open Questions

- Do we expose Grafana panels inline or via deep-link? (Impacts CSP + iframe hardening.)
- Should we host the UI on the same domain as Croniq.Api to reuse cookies, or isolate it on `ui.croniq.dev` for stricter CSP?
- Is tenant impersonation required on day one, or can we defer until the support runbook is ready?
- How aggressively should we prefetch data when operators hover over job links (trade-off between responsiveness and API load)?
