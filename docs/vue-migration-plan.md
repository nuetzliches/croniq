> **Superseded (2026-09-09).** This document plans a *parity port*, with "looks
> like it did before" as the acceptance criterion. What was decided instead was
> a **rebuild with a new design** — see
> [ADR-0004](adr/0004-vue-rebuild-for-the-dashboard.md).
>
> What still holds: the library mapping, the named risks (vue-query reactivity,
> the ConsolePage buffer, auth-guard semantics) and the drift rules. What no
> longer holds: **the effort estimate** (40–48 person-days for 9,900 lines of
> TSX — the tree had grown ~42% since, and a rebuild has no parity criterion in
> the first place), the phase plan from Phase 2 onwards, and the assumption of
> 6 UI commits a week (measured: 2.7). Phase 0 is done: Playwright suite (#586),
> dead dependencies (#583), CSS collisions (#584), the SSE core (#585).
>
> Kept as a record of what was weighed. The tree it describes as `ui/` is the
> React one, removed in v0.38.0; `ui/` now holds the Vue rebuild.

# Migration plan: the dashboard from React to Vue 3 (Tailwind stays)

As of 2026-06-11. Based on a full inventory of the codebase (55 TSX files,
~9,900 lines of TSX, 2,232 lines of CSS, 64 query/mutation hooks, 2 hand-built
SSE clients), a web-verified library mapping, and an assessment of three
migration strategies.

## Starting position — what makes this easier than expected

- **Tailwind CSS v4 is already in use** (its own `@theme` token system in
  `ui/src/index.css`, dark mode). "Move to Tailwind" does not arise — the
  migration is a pure framework change, React → Vue. `tokens.css`, `shell.css`
  and `login.css` carry over unchanged.
- **The Rust server is framework-agnostic**: `croniq-server` serves a `dist/`
  directory and nothing more (`--ui-dir`, ServeDir plus an index.html fallback,
  `main.rs:764-769`). The cutover is literally a change of COPY path in the
  Dockerfile.
- **The WASM bridge** (`croniq-config-wasm`, the DSL parser) and its build
  contract (`scripts/build-wasm.mjs`, the `predev`/`prebuild` hooks, the
  `src/lib/wasm/` target path) are framework-free and stay as they are.
- **A framework-free core**: `api/types.ts` (416 lines), `api/client.ts` (~95%),
  `lib/croniq-dsl.ts`, `lib/env.ts` and `lib/utils.ts` port by copying.

Against that: there are **no UI tests at all** (CI checks lint, tsc, build and
the WASM budget), and the repository lands about 6 UI commits a week — feature
drift during the migration is the dominant risk, not the technology.

## Library mapping (mid-2026, web-verified)

| React (today) | Vue (target) | Note |
|---|---|---|
| `@radix-ui/react-*` (Dialog, Tooltip; Switch and DropdownMenu unused) | **Reka UI v2.8** | The Radix successor for Vue. Anatomy and `data-state` attributes are near enough 1:1 that Tailwind selectors like `data-[state=open]:` port unchanged |
| `@tanstack/react-query` v5 | **`@tanstack/vue-query` v5** | The same v5 API; parameters must be passed as `computed`/`MaybeRefOrGetter` (the single biggest source of mistakes — see Risks) |
| `react-router` v7 | **vue-router v5** | A route array rather than JSX routes, guards rather than `<ProtectedRoute>` |
| `zustand` v5 | **Pinia v3** | Setup stores; `pinia-plugin-persistedstate` for the sidebar |
| `react-hook-form` v7 | **plain `reactive()` + v-model**, vee-validate only where needed | The forms are three to five fields. The react-hook-form special cases (the TimezoneInput prototype hack, the hidden-field bridge) disappear with v-model and need no replacement |
| `recharts` v3 | **removed** | A dead dependency — the charts are hand-built SVG primitives (Sparkline, Donut, HeatCell) that port 1:1 |
| `lucide-react` | **`lucide-vue-next`** | Same icon names; search and replace |
| `clsx`, `tailwind-merge`, `qrcode` | unchanged | Framework-free |
| `@vitejs/plugin-react` | **`@vitejs/plugin-vue`** | Runs on Vite 8 / rolldown; rewrite `codeSplitting.groups` onto the Vue packages |
| `tsc --noEmit` | **`vue-tsc --noEmit`** | Verify vue-tsc 3.3.x against TypeScript ~6.0.2 as a day-one spike; fallback is pinning TS 5.9 for the check alone |

**Component strategy: headless (Reka UI) plus the existing Tailwind tokens.**
No full component library (Nuxt UI v4, PrimeVue): each brings its own
theming and token system, which would then have to be maintained against the
tokens already here — and the app really uses only two Radix primitives.
shadcn-vue is worth using selectively as a scaffold reference, since it
generates Reka UI components with Tailwind v4 support into your own repository.

## Recommended strategy

**A big-bang port in a parallel `ui-vue/` directory**, enriched with the cheap
foundation pieces of the "foundation first" approach. Three strategies were
planned out and weighed against each other (big-bang in parallel, strangler /
incremental, foundation-first with a hard cutover). Big-bang wins for this
particular setup:

- One developer; the production `ui/` stays releasable throughout; no
  dual-framework operation. A strangler costs 10–15 person-days of pure
  infrastructure — a duplicated shell, duplicated `hooks.ts` maintenance,
  path-based splitting in the Rust server, a full reload at every boundary
  between the two worlds. For a densely cross-linked ten-page SPA that is the
  wrong approach.
- The cutover is a single commit, trivially reversible (the Dockerfile COPY
  path).
- ~10k lines with a central data layer (one `hooks.ts`, three small stores) sit
  in the sweet spot for a compact complete port.

**Realistic effort: about 40–48 person-days** for an experienced developer with
AI assistance, or roughly 7–9 calendar weeks. AI speeds up the mechanical
porting; the bottleneck stays manual verification, which is why Phase 0 (the
safety net) is not optional.

## Phase plan

### Phase 0 — safety net and foundation inside the existing `ui/` (5–7 person-days)

Everything here is an ordinary, immediately mergeable PR that also benefits the
React app, so abandoning the migration is not a total loss.

1. **A Playwright smoke suite** (~12–15 scenarios) against the running React
   dashboard: login (password flow), the dashboard KPIs, the `/jobs/:jobKey`
   deep link, the executions filters `?state&job_key`, dead-letter replay, the
   runners SSE indicator, the console tail, settings `?tab`, the theme toggle,
   sidebar persistence, and **session continuity** (existing `croniq_token`,
   `_refresh`, `_theme` and `_sidebar` values survive). This suite is the
   acceptance criterion for the cutover.
2. **Extract the framework-free core** (~3–4 person-days; it shrinks the surface
   to be ported):
   - `lib/auth-token.ts`: a token holder with no zustand import, decoupling
     `client.ts` from the store. Pinia later attaches identically.
   - `lib/sse.ts` (`createSseStream`): ONE tested SSE core (fetch streaming,
     backoff, abort) for `useRunnersSSE` and ConsolePage — imported rather than
     ported twice.
   - `lib/api-error.ts` (`parseApiError`): a single place that parses errors,
     for client.ts, toast.ts and LoginPage.
   - The theme FOUC bootstrap as an inline script in `index.html` — guaranteed
     to survive the framework change.
   - A vitest setup with ~25–35 unit tests for that core (bearer/401/204, SSE
     chunk boundaries, the error envelope, theme resolution).
3. **Clear out what is dead**: remove dead dependencies (`recharts`,
   `@radix-ui/react-switch`, `@radix-ui/react-dropdown-menu`, plus the recharts
   chunk rule); extract `DeliveriesList` out of `AlertsPage.tsx` into
   `components/`, which resolves the cross-page import from JobsPage.
4. **Fix the CSS collisions in the live tree**, where the running app is
   available as an oracle: `components.css` defines `.grid`, `.gap-4/6/8/10/14`,
   `.grow` and `.cols-*` as unlayered CSS and so overrides Tailwind utilities —
   `grid grid-cols-7 gap-1` in the ScheduleBuilder really gets `gap:14px`.
   Rename them or move them into `@layer`; delete the dead CSS (~150–200 lines).
5. **PORTING-NOTES.md**: a checklist of every silent contract — dynamic class
   names (`lvl-${lvl}`, `kpi-delta ${direction}`, `audit-${kind}`), data
   attributes (`.app[data-sidebar]`, `.sidebar[data-collapsed]`,
   `aria-expanded`), localStorage keys, URL contracts, query-key quirks (the
   singular/plural `dead-letter`/`dead-letters`), the JobsPage implicit first
   selection, the `?tab` allowlist, and 409/400 error parsing. This becomes the
   binding acceptance checklist, page by page.
6. **Toolchain spikes (go/no-go before Phase 1)**: `@vitejs/plugin-vue` on
   rolldown-Vite 8; `vue-tsc` 3.3.x against TS ~6.0.2. Fallbacks: pin TS 5.9 for
   the check, and give up manual chunking if it comes to that.

### Phase 1 — the `ui-vue/` scaffold and data layer (5–6 person-days)

- A Vite 8 project with `@vitejs/plugin-vue`, the `@→src` alias, an identical
  dev proxy (`/v1`, `/health`, `/metrics`), and the WASM hooks wired up
  unchanged.
- Straight copies: `types.ts`, `utils.ts`, `env.ts`, `croniq-dsl.ts`,
  `tokens.css`, `shell.css`, `login.css`, and the cleaned-up `components.css`;
  `client.ts` with `auth-token.ts` from Phase 0.
- Consolidate dark mode onto ONE system: map the `@theme` hex tokens onto the
  oklch variables, and use `[data-theme]` alone rather than an additional
  `.dark` class.
- Three Pinia stores (auth, sidebar, toast) with **identical localStorage keys**,
  so existing sessions survive the cutover.
- **`hooks.ts` (772 lines) in ONE focused PR** onto vue-query: all 26 queries
  and 38 mutations, query keys byte-identical, and **every parameter as a
  `computed`/`MaybeRefOrGetter`** — a hard convention, with a review checklist
  per hook. Replicate the global `MutationCache.onError` (`meta.action`, the
  'Unauthorized' silencing).

### Phase 2 — design system: primitives, ui components, dialogs, builders (7–9 person-days)

- 13 trivial primitives as SFCs (ReactNode props become slots, `useId` for SVG
  mask ids); consolidate the duplicates (EmptyState 2→1, CopyBtn 2→1).
- The ui base: Button (forwardRef disappears), Badge, Card, RelativeTime (VueUse
  `useNow`), DataTable with **scoped slots** rather than cell render props
  (`<script setup generic="T">`).
- A Reka UI base dialog plus **`useConfirm` as a composable with a globally
  mounted `ConfirmHost`** (Teleport) — replacing the React pattern of "a hook
  returns JSX" centrally for all eight-plus call sites.
- **TimezoneInput (281 lines, the single largest risk)**: the
  HTMLInputElement prototype-setter hack for react-hook-form disappears in
  favour of v-model; `createPortal` becomes Teleport with scroll/resize
  tracking; combobox behaviour inside a dialog needs manual testing.
- ScheduleBuilder and CalendarRuleBuilder: `croniq-dsl.ts` stays 1:1, but every
  `useEffect` cancelled flag becomes a disciplined `watch` plus `onCleanup` —
  otherwise fast typing races the WASM parse. Callbacks become emits and
  v-model.
- Dialogs (EditJob, NewJob, Schedule): plain `reactive()` plus v-model. Make the
  decision in the first dialog and carry it through.

### Phase 3 — shell, routing, auth, LoginPage (4–6 person-days)

- The vue-router route table 1:1 from `App.tsx` (history mode, lazy imports per
  route).
- **Guard auth twice**: a `beforeEach` guard (`meta.requiresAuth`) AND a `watch`
  on `isAuthenticated` that calls `router.replace('/login')`. Vue guards fire
  only on navigation; without the watch the page just sits there after a 401
  logout. This is the most reactive difference from React's `<Navigate>`
  pattern.
- Shell SFCs with **exact DOM and attribute parity**
  (`.app[data-sidebar=collapsed]`, `.sidebar[data-collapsed]`,
  `.user-pill[aria-expanded]`) — `shell.css` selects on those and breaks
  silently otherwise. The chunk-loading spinner goes on the router loading
  state; Vue Suspense is not the tool for it.
- LoginPage (1,091 lines): port the auth flow including TOTP enrolment
  carefully, 1:1. Rewrite the demo console state machine and the verb rotation
  as standalone timer composables rather than translating them line by line.
  TOTP enrolment becomes a shared component (LoginPage plus ProfileTab).

### Phase 4 — port the pages in ascending order of difficulty (9–13 person-days)

Run both dashboards side by side (React on :4231, Vue on :4232, the same API),
accept each page individually, use PORTING-NOTES as the gate, and get the
Playwright specs for that page green:

1. **DashboardPage** (368 lines, purely declarative) — validates the data layer
   and the primitives
2. **ExecutionsPage** — establishes the URL sync pattern (`route.query` plus
   `router.replace`)
3. **DeadLettersPage** — master/detail, swallowed 404s, the replay banner
4. **RunnersPage** — the first SSE integration under adverse conditions (kill
   the server, watch the backoff)
5. **CalendarsPage** — CalendarRuleBuilder; the react-hook-form hidden-field
   bridge disappears
6. **SettingsPage and its tabs** — the `?tab` allowlist, the role gate becomes
   `v-if`
7. **AlertsPage**
8. **JobsPage** (1,245 lines, 16 hooks, the heaviest of them) — keep the
   implicit first selection when there is no `:jobKey`, and make the KpiRow
   countdown a `useNow` composable
9. **ConsolePage** — the event buffer as a `shallowRef` with manual
   `triggerRef`, and pending/paused as non-reactive variables. Load-test with
   2,000 events: a naive `ref([])` port makes every SSE event a deep-reactivity
   trigger

### Phase 5 — CI and Docker cutover, acceptance, teardown (3–4 person-days)

- **Do not rename the `UI (build + typecheck)` CI job** (it is a required status
  check — incident #98). Swap the steps inside it (`tsc` → `vue-tsc`), and add
  Playwright as a new job that is not required at first.
- **The cutover is one PR**: Dockerfile stage 2 and the runtime COPY move to
  `ui-vue/`; bring the WASM COPY path along. Rollback is a revert, or the
  previous Docker image tag.
- Acceptance: the Playwright suite green against the Docker image; a parity
  review against PORTING-NOTES; session continuity (staying logged in across
  the cutover); a FOUC test in light, dark and auto.
- After one or two weeks of observation — especially the console and runners SSE
  under load — a teardown PR: delete `ui/`, move `ui-vue/` to `ui/`, remove the
  React dependencies.

## Drift rules during the parallel phase

At about 6 UI commits a week, feature drift is the main risk. Budget 3–6
person-days for it if a freeze is not possible.

- Aim for a UI feature freeze during the hot phase (4–6 weeks) and put a date
  on it.
- Once a file has been ported: **fix in the Vue tree first, cherry-pick to
  React** — never the other way round.
- Keep a frozen list per ported file, with a PR checklist: "does this touch the
  shell or hooks? → then both trees."

## Top risks

| Risk | Countermeasure |
|---|---|
| The vue-query reactivity trap: parameterised hooks freeze on a filter or route change when the keys are not passed as `computed` | hooks.ts in one PR with a hard convention; Playwright flows for filter changes |
| Feature drift against the live `ui/` | The drift rules above; pull the foundation forward, freeze only for Phases 1–5 |
| `shell.css` and `components.css` select on data attributes and dynamic class names, and break silently | The PORTING-NOTES checklist as an acceptance gate, page by page |
| ConsolePage performance (a 2,000-event buffer) | `shallowRef` plus non-reactive buffers, load-tested before acceptance |
| Toolchain unknowns (plugin-vue/rolldown, vue-tsc/TS 6) | Day-one spikes with defined fallbacks, before any porting effort is spent |
| A required status check blocks merges when a job is renamed | Freeze the job names, swap only the steps |
| The reactive 401 redirect is lost, because guards fire only on navigation | An `isAuthenticated` watch plus a dedicated end-to-end test |

## To decide before starting

1. **Why Vue?** 40–48 person-days with no visible benefit to an end user. The
   strategic reasoning — team skills, maintainability, ecosystem — should be
   written down before Phase 0 starts.
2. **Feature freeze**: is a four-to-six-week window enforceable, or do we work
   with a drift budget (3–6 person-days)?
3. **Parity versus consolidation**: the recommendation is to consolidate only
   the CSS collisions and dark mode in the React tree beforehand. Every other
   redesign (the DataTable API, useConfirm) is part of the port, and purely
   optional unifications (query-key names) wait until after the cutover.
4. **Acceptance criterion**: Playwright smoke plus a manual side-by-side
   acceptance per page (recommended) — not a full screenshot-diff apparatus,
   which is flaky from animations, portals and fonts, and oversized for a
   one-developer project.
5. **Rollback threshold**: how long does the React tree stay in the repository
   (one or two releases), and which class of fault triggers a rollback rather
   than a forward fix?
6. **Playwright as a required check** after the migration. Closing the testing
   gap permanently is the most valuable side effect of the project and should
   not be allowed to quietly lapse.
