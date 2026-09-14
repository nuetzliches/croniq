# ADR-0004: Rebuild the dashboard in Vue 3

- **Status:** Accepted; carried out 2026-09-14 (see *Cutover*)
- **Date:** 2026-09-09
- **Supersedes:** [ADR-0003](0003-react-for-the-dashboard.md)
- **Related:** [#589](https://github.com/nuetzliches/croniq/issues/589), [#595](https://github.com/nuetzliches/croniq/issues/595), `docs/ui-architecture-decision.md`, `docs/vue-migration-plan.md` (superseded)

## Context

[ADR-0003](0003-react-for-the-dashboard.md) recorded React 19 + TypeScript,
chosen in 2026-04 and never written down until it was questioned. The question
came up because the organisation's other frontends — `nuts-customer-portal` and
`ciphr/ui` — are Vue, and croniq is the outlier. For a one-person operation the
context-switching cost is recurring and falls on the same person every time.

The analysis in `docs/ui-architecture-decision.md` recommended staying on React
for now, on the grounds that a port is expensive and returns nothing an end
user can see. That recommendation was made, considered, and not taken. The
consolidation argument was judged to outweigh it. This ADR records the decision
that was actually made, not the one that was recommended.

Two facts shape what follows. First, the dashboard's design is being rebuilt
rather than ported — so this is not the migration `docs/vue-migration-plan.md`
(2026-06-11) planned, and that document's 40–48 person-day estimate does not
apply. It costed a *parity* port with "looks the same" as the acceptance
criterion, against a tree that has since grown ~42%. Second, a rebuild has no
parity criterion at all, which removes the natural boundary a port has. That is
the main risk here, and it is answered under *Consequences*.

## Decision

The dashboard is rebuilt in Vue 3, with a new design, in a parallel tree. The
React dashboard under `ui/` stays the shipping dashboard until the new one
passes the acceptance gate below, at which point the cutover is a build path
change and the React tree is removed.

*This happened on 2026-09-14; see [Cutover](#cutover).* The text of this
section is left in the future tense because it is the decision as taken, and
rewriting a decision record to match its outcome loses the only thing it was
for.

The rebuild starts with a design phase — which screens exist and what design
system they use — because a rebuild without that is a redesign performed one
component at a time.

## Alternatives considered

- **Stay on React** (ADR-0003 confirmed). The recommendation in
  `docs/ui-architecture-decision.md`: React 19 + Vite 8 + Tailwind 4 is a
  current, maintained base under no forcing function, and the work returns
  nothing an end user can see. Rejected: it settles the outlier problem
  permanently in the wrong direction, and the cost of context-switching is paid
  on every future change rather than once.
- **Parity port to Vue** (Reka UI headless + the existing oklch tokens), as
  planned in `docs/vue-migration-plan.md`. Rejected because the design is being
  rebuilt anyway; porting markup that is about to be replaced is work performed
  twice.
- **Redesign in React first, migrate later.** Would keep one variable at a
  time, which is the textbook advice. Rejected: it means touching every screen
  twice for the same outcome.

## Consequences

- **The cost is not the migration plan's 40–48 person-days.** That number is
  for a parity port against a smaller tree. A rebuild is mechanically cheaper
  per screen in places (no pixel matching) and unbounded in scope everywhere
  else. Treat any estimate carried over from that document as wrong.

- **Scope guard, because a rebuild has no natural edge.** Three rules, all of
  which exist to make "done" answerable:

  1. **The capability list is frozen at what exists.** New features wait for
     after the cutover; a rebuild that also grows the product has no completion
     criterion. *Amended 2026-09-10:* this first read "the screen list is
     frozen at the ten routes the React dashboard serves today", which was too
     literal — the screens are being deliberately re-cut and merged, and
     merging is the opposite of scope growth. What is frozen is the set of
     capabilities, enumerated in `docs/ui-screen-inventory.md` under *Was nicht
     verloren gehen darf*; the arrangement is open.
  2. **The acceptance gate is the Playwright suite** (`ui/e2e/`, #586). It
     asserts routes, the login and refresh-cookie session behaviour, the URL
     contracts, both SSE surfaces, and preference persistence — all of it
     framework-agnostic and none of it about appearance. It must pass against
     the Vue build before cutover. It survives the rebuild unchanged; the
     computed-style snapshot tool (`ui/scripts/style-snapshot.mjs`, #584) does
     not, since there is deliberately nothing to keep identical.
     *Amended 2026-09-11:* until
     [#620](https://github.com/nuetzliches/croniq/issues/620) this guard was
     aspirational — the suite ran only against the React build the server
     serves, so the gate was measuring the tree being replaced. It now runs
     against both, one per invocation (`CRONIQ_E2E_TREE`), and CI does both on
     every push. The three places the two dashboards genuinely promise
     different things are named in `ui/e2e/trees.ts` rather than duplicated
     across specs; the React half of that file leaves with the React tree.
     *Amended 2026-09-14:* it did. `trees.ts` is gone, the switch with it, and
     what the specs needed from it — the nav list, sign-out, the theme picker —
     is now `ui/e2e/app.ts`, describing one dashboard rather than
     reconciling two.
  3. **Fixes go to the Vue tree first** once a file is rebuilt, and are
     cherry-picked back if the React tree still needs them. Never the reverse.
     *Amended 2026-09-11:* the cherry-pick half is withdrawn. The React tree
     receives no further work of any kind and is removed in full at the
     cutover, so a fix there is thrown away by definition. This came up as a
     real decision rather than a hypothetical: the job DSL view was found to
     emit text that croniq's own lexer rejects
     ([#624](https://github.com/nuetzliches/croniq/pull/624) fixed it in Vue,
     [#625](https://github.com/nuetzliches/croniq/issues/625) proposed the same
     fix for React and was declined). Expect the argument to recur on each
     defect found while porting a screen, and expect it to be tempting each
     time — the answer is that the shipping dashboard is a dead tree and
     patching it spends effort that the cutover erases.

- **[#595](https://github.com/nuetzliches/croniq/issues/595) folds in.** 51 of
  54 form controls have no accessible name, and `UserMenu` is a `role="menu"`
  with no `menuitem` children. Fixing that in React would be discarded markup.
  It becomes an acceptance criterion for the rebuild instead: every control
  gets an accessible name as it is written, and the ARIA containers are valid.
  The cost of this choice is that the shipping dashboard stays inaccessible for
  the duration of the rebuild — which is a real cost to real users and is the
  strongest argument the "stay on React" option had.

- **The design system is now a decision, not an inheritance.** The React tree's
  oklch token system in `ui/src/styles/` is project-owned. `nuts-customer-portal`
  uses Nuxt UI 4, which brings its own theming. Whichever way this goes, it is
  a product decision about how Croniq looks and should be made explicitly in
  the design phase rather than falling out of a library choice.

- **Drift during the parallel phase.** UI churn measured 2.7 commits/week over
  the 90 days to 2026-09-09 — half what the June plan assumed, so the risk is
  smaller than that document claims, but it is not zero.

- **The Rust side is unaffected.** `croniq-server` serves a directory
  (`--ui-dir`, `croniq_server::ui_assets`) and does not know what built it. The
  WASM bridge (`crates/croniq-config-wasm`) and its build contract
  (`scripts/build-wasm.mjs`) are framework-free and carry over unchanged.

## Cutover

Carried out 2026-09-14, after the capability list in `docs/ui-screen-inventory.md`
was satisfied in full and the Playwright suite passed against the Vue build.
What moved, in one change:

- The React tree in `ui/` deleted — 118 files. Nothing was kept "just in
  case": git has it, and a parallel tree nobody builds is a tree that rots into
  a trap.
- The e2e suite, the Playwright config and the seven tooling scripts moved out
  of it and into the rebuild, which is now the only npm project in the repo
  besides the SDKs. `style-snapshot.mjs` was not moved: guard 2 above says it
  does not survive a rebuild, and it did not.
- **The rebuild then moved into `ui/`.** It grew up in `ui-vue/` because the
  name had to be free while both trees existed; keeping that name afterwards
  would leave the directory encoding a framework, which is the exact thing
  [ADR-0003](0003-react-for-the-dashboard.md) got caught by — a stack recorded
  in a name nobody revisits. `ui-vue/` no longer exists. Pure renames, so
  history follows.
- `Dockerfile`, `.github/workflows/*`, `scripts/dev-stack.mjs`, `.gitignore`
  and `.dockerignore` follow the directory. The CI job kept the name
  `UI (build + typecheck)` and swapped its contents, because it is a required
  status check and renaming one blocks every merge until branch protection
  catches up (incident #98).
- The dashboard moved back to **4231**, the UI slot in the 4230-4233
  development block; 4232 is free. It only ever sat on 4232 because the tree it
  was replacing held 4231.
- `VITE_API_URL` and its `VITE_ALLOW_LOCALSTORAGE_REFRESH` guard are gone with
  the React tree. The rebuilt dashboard is same-origin only, which makes
  [ADR-0001](0001-same-origin-dashboard.md) hold by construction instead of by
  a build-time check; that ADR is amended accordingly.

Two things this cutover did **not** do, deliberately. It did not rewrite the
historical documents — `docs/ui-architecture-decision.md`, `docs/ui-spike-brief.md`,
`docs/reviews/*` and ADR-0003 still describe `ui/` in the present tense,
because they are records of when that was true. Note the trap the rename
leaves behind: those documents name a path that still exists and now holds
something else. Anything written before 2026-09-14 that points at `ui/` means
the React tree. And the cutover did not renumber, rename or re-scope anything
that was merely adjacent to it.

## Enforced by

- `AGENTS.md` → "Core Expectations", item 1
- `ui/package.json`, `ui/vite.config.ts` — the toolchain
- `.github/workflows/ci.yml` — `UI (build + typecheck)` and `UI (e2e smoke)`.
  Do not rename `UI (build + typecheck)`: it is a required status check, and
  renaming one blocks every merge until branch protection is updated
  (incident #98). Swap its steps instead.

## What this ADR does not say

It did not name the design system, the component library, or the screen
inventory — those were the design phase's output. That phase has since chosen
the `nuts-customer-portal` stack (Vue 3 + Nuxt UI 4 + Pinia + vue-query) and a
re-cut screen set; both live in `docs/ui-screen-inventory.md`, which is the
document to read before writing any of it.

It does not set a date. The rebuild competes with everything else for the same
single developer, and the scope guard above matters more than a schedule.
