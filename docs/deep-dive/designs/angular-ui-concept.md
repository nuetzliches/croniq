# Croniq UI Concept (Angular 21 + Tailwind)

## Goals & Success Criteria

- Ship an opinionated admin UI that mirrors the personas and flows listed in `docs/deep-dive/ui.md` while staying optional for headless Croniq deployments.
- Keep the UI repo-local (no separate SPA repo) so architecture reviews and CI/CD remain aligned with backend changes.
- Deliver a design language that feels operational, dense, and telemetry-aware; dashboards must communicate queue state, policy health, and webhook posture without feeling like a generic CRUD generator.

## Guardrails & Dependencies

- Backend prerequisites: stable schedule/job/admin APIs, gRPC-Web proxy (if needed), finalized OIDC story, and observability endpoints as called out in `docs/deep-dive/architecture.md` and `docs/deep-dive/ui.md`.
- Platform constraints: MIT/Apache/BSD-only dependencies, OpenTelemetry-first instrumentation, strict separation between secrets and UI bundles (no API keys in browser storage).
- Hosting: built artifacts deploy either as static assets behind Croniq.Api or via a slim `Croniq.Ui` container image that speaks the same readiness/liveness protocol as other services.

## Technology Decisions

| Concern            | Decision                                                                                 | Notes                                                                                                                                                                                                    |
| ------------------ | ---------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Framework          | Angular 21 standalone apps, built with the Vite-powered builder                          | Gives SSR-ready hydration, Signals API, and CLI ergonomics familiar to enterprise contributors.                                                                                                          |
| Styling            | Tailwind CSS + custom tokens                                                             | Tailwind provides utility primitives; we layer Croniq-specific tokens (spacing, typography, semantic colors) via `tailwind.config.ts` and CSS variables for dark/light surfaces.                         |
| Component strategy | Headless primitives + lightweight shims                                                  | Compose Radix-inspired headless patterns with Tailwind classes to avoid Material sameness; focus on split-pane layouts, dense data grids, and status pills.                                              |
| State/query        | Angular Query (TanStack Query for Angular) + `effect` based local stores                 | Query handles caching, retries, and background refresh; Signals/effects model UI-only state.                                                                                                             |
| Forms              | Angular Reactive Forms + Zod schema validation compiled via `@abraham/reflex` or similar | Keeps validation logic shareable with backend contracts.                                                                                                                                                 |
| Testing            | Vitest + Playwright + Storybook (Chromatic optional)                                     | Aligns with Croniq testing stack; Playwright exercises auth + data grid behavior.                                                                                                                        |
| DX automation      | Angular MCP Server                                                                       | Allows VS Code + GPT-5.1-Codex agents to scaffold components/modules that respect the repo-level AI instructions; server remains optional and runs locally so no build/runtime dependency is introduced. |

## Repository & Project Layout

```
src/
  Croniq.Ui/
    angular.json
    package.json
    tailwind.config.ts
    tsconfig.base.json
    apps/
      admin/
        src/
          app/
            core/        # auth, api clients, guards
            shared/      # reusable UI atoms/molecules
            features/
              dashboard/
              schedules/
              jobs/
              webhooks/
              tenants/
            app.config.ts
            app.routes.ts
          environments/
    libs/
      data-access/       # typed API clients, DTO mapping helpers
      telemetry/         # OTEL bridge, log ingestion helpers
      ui-kit/            # headless components styled w/ Tailwind tokens
```

- Place the Angular workspace in `src/Croniq.Ui` so it lives next to other product code and inherits the same build/versioning pipelines.
- Use secondary entry points (libraries) to separate regulated surfaces (telemetry, auth) from visual components; this aides tree-shaking when we publish micro-frontends later.
- Tailwind config exports CSS variables under the `:root[data-theme="ops"]` namespace to express semantic colors (`--cq-surface`, `--cq-accent`, `--cq-danger`) and follows the official Angular Tailwind guidance at [https://next.angular.dev/guide/tailwind](https://next.angular.dev/guide/tailwind) for builder integration and content scanning configuration.

## Application Architecture

1. **Shell & Layout**

   - Persistent command rail on the left with tenant selector + status beacons (cluster health, clock skew, policy alerts).
   - Main canvas uses split panes (top summary cards, lower tabbed details) to emphasize live metrics.
   - Global command palette (Ctrl+K) surfaces job search, quick trigger, and navigation actions.

2. **Feature Modules**

   - `dashboard`: queue depth spark lines, upcoming triggers, misfire heat map (ECharts or visx).
   - `schedules`: list + detail editing surface with JSON diff viewer so operators can see policy deltas before saving.
   - `jobs`: registry browser, ability to trigger jobs manually, show last N executions.
   - `webhooks`: ingress status, secret rotation controls, IP allow-list grid.
   - `tenants & API keys`: manage quotas, rotate keys, view policy overrides.

3. **Data Access**

   - Generate REST clients via OpenAPI (NSwag) or call the existing `Croniq.Rpc.Client` through a lightweight WebAssembly proxy if gRPC-Web is required.
   - Central `ApiClient` service injects the auth token/API key and adds OpenTelemetry trace headers so UI actions appear in distributed traces.
   - Use Angular Query to automatically refetch stale data when the operator switches tenants or env tags.

4. **State & Caching**
   - Use Signals for local ephemeral state (panel toggles, wizard steps) to avoid unnecessary RxJS complexity.
   - Persist user preferences (theme, table density) via IndexedDB + encryption where possible; treat as non-sensitive but namespaced per tenant.

## Styling & Design Language

- Typography: pair `Space Grotesk` (headings) with `IBM Plex Mono` (metrics) to create a console-inspired look.
- Color palette: deep charcoal backgrounds with saturated amber/cyan accents to highlight policy breaches or successful triggers.
- Motion: prefer intentful transitions (panel sweep, counter flip) over micro-animations. Global loading indicators appear as progress beams at the top edge.
- Layout primitives: Tailwind utility classes plus a `stack`/`cluster` set of CSS components to enforce spacing rhythm (8px grid).

## Security & Auth Integration

- Support OIDC PKCE login for human operators; fallback to short-lived API tokens for service technicians behind VPN.
- Never persist secrets locally; rely on backend-managed session cookies or ephemeral tokens stored in memory.
- Integrate with `ICallerContext` metadata so the UI can show "acting as" context and log audit trails for manual actions.
- Enforce feature flags in the backend as well; the UI only exposes toggles that the API already guards.

## Tooling & MCP Server Usage

- The Angular MCP Server runs alongside VS Code so GPT-5.1-Codex agents can execute workspace-aware tasks (create components, update routes) while honoring the guardrails from `AI_ASSISTANT_INSTRUCTIONS.md`.
- MCP stays dev-only: no runtime dependency, no shipped assets. Document how to start it via `npm run mcp` and expose a `.vscode/tasks.json` helper.
- Use the server to codify scaffolding recipes (e.g., `generate feature schedules --with-crud --with-grid`), ensuring consistent folder structure/tests.
- Align MCP prompts and automation scripts with Angular's AI guidance: leverage the "Develop with AI" workflows [https://next.angular.dev/ai/develop-with-ai](https://next.angular.dev/ai/develop-with-ai) to keep generated code idiomatic, and apply the recommended AI design patterns [https://next.angular.dev/ai/design-patterns](https://next.angular.dev/ai/design-patterns) to validate what the agent produces before committing it.

## Build, Test & Release Flow

1. **Build**: `npm run build` uses Angular's Vite builder; Tailwind compiled during build to minimize runtime CSS cost.
2. **Static analysis**: ESLint + Angular template lint + `npm run lint:styles` for Tailwind class validation.
3. **Unit/UI tests**: Vitest for logic + DOM tests; Storybook stories double as visual regression coverage via Chromatic (optional but recommended).
4. **E2E**: Playwright suite runs against the devstack, mocking OIDC tokens via the existing test host.
5. **Packaging**: artifacts emitted to `dist/apps/admin`; publish to `eng/artifacts/ui` for CI consumption and to a container image for Ops.

## Delivery Phases

1. **Design Spike** (1 sprint)
   - Produce wireframes + updated design tokens.
   - Finalize Tailwind theme + typography approvals with stakeholders.
2. **Scaffolding & Auth** (1 sprint)
   - Initialize Angular workspace, configure MCP tasks, hook up OIDC stub + tenant switcher.
3. **MVP Data Surfaces** (2 sprints)
   - Dashboard metrics (stubbed), schedules read-only grid, job registry view.
4. **Admin Controls** (2 sprints)
   - CRUD for schedules/webhooks/API keys; integrate dead-letter replay.
5. **Observability & Polish** (2 sprints)
   - Grafana embedding, log pulse view, accessibility + localization review.

## Open Questions

- Do we expose Grafana panels inline or via deep-link? (Impacts CSP + iframe hardening.)
- Should we host the UI on the same domain as Croniq.Api to reuse cookies, or isolate it on `ui.croniq.dev` for stricter CSP?
- Is multi-tenant impersonation required on day one, or can we defer until the support runbook is ready?
- How aggressively should we prefetch data when operators hover over job links (trade-off between responsiveness and API load)?
