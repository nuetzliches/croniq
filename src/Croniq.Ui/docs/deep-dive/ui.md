# Croniq UI Backlog Plan

This document defines the scope, technology evaluation, and backlog required to fulfill the checklist item "UI-Backlog dokumentieren; Technologie nach API-Stabilisierung entscheiden".

## Objectives

- Deliver an administrative UI that surfaces core Croniq capabilities (tenants, schedules, triggers, jobs, observability) once the API and provider contracts stabilize.
- Keep the UI optional, decoupled from the scheduler runtime, and deployable as a static SPA or container.
- Provide a clear decision tree for technology choices (component libraries, charts, auth integration) so work can start as soon as backend prerequisites are done.

## Target Personas & Use Cases

1. **Platform operators**: monitor scheduler health, inspect dead-letter queues, manage tenants & API keys, view quotas/misfires.
2. **Developers/job authors**: browse job metadata, trigger manual executions, inspect logs/metrics for their namespace.
3. **Support / SRE**: investigate incidents, replay dead-lettered executions, confirm rate limits and policy overrides.

## High-Level Requirements

- Authentication: reuse API-key/OIDC flows (per `security.md`). Support tenant-scoped views + admin roles.
- Features (MVP): dashboard (queue depth, trigger throughput), schedules CRUD, job registry view, dead-letter browser, API-key manager.
- Observability visualizations: integrate with OTel metrics via backend proxy or embed Grafana panels.
- Extensibility: plugin-friendly for custom job metadata or provider-specific actions.

## Technology Options

| Frontend Stack                       | Pros                                                             | Cons                                                          | Status            |
| ------------------------------------ | ---------------------------------------------------------------- | ------------------------------------------------------------- | ----------------- |
| Angular 21 + Vite + TypeScript       | Opinionated enterprise tooling, CLI scaffolding, RxJS-friendly   | Heavier runtime than Svelte, contributors need Angular skills | Preferred default |
| React + Vite + TypeScript + Tailwind | Large ecosystem, easy component sharing, good DX                 | Common choice, but needs careful design to avoid generic look | Alternative       |
| SvelteKit                            | Lightweight, excellent transitions, good for dashboard-style UIs | Smaller pool of contributors                                  | Alternative       |
| Blazor WebAssembly                   | .NET-native, reuses models                                       | Larger payloads, requires .NET runtime download               | Hold              |

- Component library candidates: Radix UI + custom styling, Chakra UI, or custom design system. Avoid Material clones to keep a distinct visual identity.
- Charts: `visx`, `ECharts`, or `Recharts` (React) to plot queue depth and schedule stats.

## Architecture Outline

- UI is a standalone SPA served from CDN or Croniq.Api static hosting.
- Data access via Croniq REST/gRPC endpoints (use backend proxy for gRPC-Web if needed).
- State management: React Query/TanStack Query for data fetching; keep local stores minimal.
- Auth: API keys stored in browser? Avoid; prefer user tokens (OIDC). For admin flows, run UI behind an internal gateway injecting tokens or use PKCE auth code flow.
- Feature flags: integrate with existing configuration (headers or query) to toggle advanced features.

## Webhook IP Allow-List Surface

- Add a tenant-scoped grid for webhook endpoints showing the current CIDR list from `GET /tenants/{tenantId}/webhooks/{hookKey}/ip-rules`.
- Support inline create/delete actions with optimistic updates so operators can reconcile their CMDB inventory quickly.
- Display audit metadata (`CreatedBy`, timestamps) and expose CSV/JSON export to simplify reviews with security teams.
- Highlight enforcement state per hook (open vs locked down) and warn when production hooks lack any CIDRs.
- Reuse the same helper layer that the SDK will expose so UI + automation stay aligned.

## Delivery Phases

1. **Design & Wireframes**: define IA, layout, color scheme, navigation. Produce Figma or equivalent.
2. **Scaffolding**: create `src/Croniq.Ui` (Angular 21 + TypeScript) with linting, tests, Storybook/Storybook-like tooling, CI build.
3. **MVP features**: login flow, dashboard (metrics stub), schedules list/detail (read-only), job registry view.
4. **Management features**: API-key CRUD, tenant admin, trigger creation, manual job trigger UI.
5. **Observability integration**: embed metrics/traces (via OTel backend or Grafana). Provide log viewer hooking into Serilog sinks.
6. **Polish & release**: theming, accessibility, localization (if required), packaging as static assets + Dockerfile.

## Dependencies & Prereqs

- Stable API contract (`/tenants/{tenantId}/schedules`, `/jobs`, future admin endpoints) + OIDC integration.
- Policy, observability, and dev stack milestones complete (UI depends on their data feeds).
- Decide hosting target (same repo vs separate). Recommendation: new project `src/Croniq.Ui` with optional publishing to `ui/` folder or container image.
- Sequencing: do not begin UI implementation until all backend/provider/observability/security milestones are complete; UI remains a downstream stream after Core/API readiness.

## Backlog to Complete Checklist Item

- [ ] Draft IA/wireframes and attach to `src/Croniq.Ui/docs/deep-dive/ui.md` (link to design artifacts).
- [ ] Implement MVP dashboard + schedules read-only views using mocked data, then wire to API once ready.
- [ ] Add webhook IP allow-list management UI (list/create/delete, diff/export) once the SDK helper ships.
- [ ] Document contribution guidelines (coding standards, CSS strategy, design tokens) and add to docstreams plan.
- [ ] Provide deployment strategy (static hosting + Dockerfile) and integrate into release pipeline.

This plan can be expanded as soon as prerequisites are satisfied. Completing the backlog will unblock the UI milestone in the checklist.
