# ADR-0003: React + TypeScript for the dashboard

- **Status:** Accepted, under review
- **Date:** 2026-04-11 (recorded 2026-09-09)
- **Related:** `docs/vue-migration-plan.md`, `docs/ui-architecture-decision.md`

## Context

The dashboard was started on 2026-04-11 (`feat(ui): add React dashboard with
Vite + Tailwind + TanStack Query`), replacing .NET remnants from the project's
first months. It has grown to 93 files and ~16.4k LOC (14.1k TS/TSX, 2.3k CSS)
across ten pages, two schedule builders backed by a WASM bridge, and two SSE
streams.

The choice was never written down. `AGENTS.md` states it as a rule — "Target
Stack — Rust, React + TypeScript for UI" — with no reasoning and no date, which
is what made it hard to weigh when the question came up again.

## Decision

The dashboard is a React 19 + TypeScript SPA, built with Vite, styled with
Tailwind CSS v4 over a project-owned oklch token system, using TanStack Query
for server state, Zustand for client state, and Radix primitives where a
headless component is needed.

## Alternatives considered

Recorded in 2026-09, so these are the alternatives as they stand now rather
than as they were weighed in April:

- **Vue 3.** Planned in detail in `docs/vue-migration-plan.md` (2026-06-11) and
  re-costed in `docs/ui-architecture-decision.md` (2026-09-09) at 55–70
  person-days against today's larger tree. The case for it is stack
  consolidation with the other projects in this organisation, not a technical
  deficiency in the current stack. Not rejected — deferred; see *Status*.
- **Server-rendered HTML (askama/maud) instead of an SPA.** Would remove the
  npm supply chain from the build entirely, which [ADR-0002](0002-single-image-delivery.md)
  names as a cost. It does not survive the live surfaces: two SSE streams, a
  keystroke-rate schedule preview through WASM, and optimistic mutation state.
- **A full component library (Nuxt UI, PrimeVue, MUI).** Rejected: it brings
  its own theming system that would have to be maintained against the existing
  tokens, and the app uses two headless primitives in total.

## Consequences

- The dashboard's dependency tree is the largest supply-chain surface in the
  repo, and it is compiled into the same image build as the server binary
  ([ADR-0002](0002-single-image-delivery.md)).
- React-specific workarounds exist and are load-bearing: an
  `HTMLInputElement`-prototype setter hack in `TimezoneInput` to satisfy
  react-hook-form, a "hook returns JSX" confirm pattern across eight-plus call
  sites, and `useEffect` cancellation flags guarding the WASM preview against
  races.
- The stack diverges from the organisation's other frontends, which are Vue.
  That cost is recurring and falls on whoever context-switches between them.

## Enforced by

- `ui/package.json`, `ui/vite.config.ts` — the toolchain
- `AGENTS.md` → "Core Expectations", item 1
- `.github/workflows/ci.yml` → the `UI (build + typecheck)` job, a required status check

## Status note — under review

The framework choice is being reconsidered as part of the UI architecture work
tracked in `docs/ui-architecture-decision.md`. The recommendation there is to
stay on React for now and instead close the test gap and split the delivery
first, because a framework port is 55–70 person-days with no end-user benefit
and no safety net today.

If Vue is adopted, this ADR is **superseded by a new one** rather than edited —
the record that React was chosen, and why the reversal was taken, is the point.
