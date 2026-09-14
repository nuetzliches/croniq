# Croniq UI

The dashboard. Vue 3, rebuilt from the React SPA that occupied this directory
until 2026-09-14, per
[ADR-0004](../docs/adr/0004-vue-rebuild-for-the-dashboard.md). The rebuild grew
up next door in `ui-vue/` and moved in once the old tree was removed, so
anything written before that date and pointing at `ui/` means the React one.

Read [`../docs/ui-screen-inventory.md`](../docs/ui-screen-inventory.md) before
adding a screen — it holds the agreed screen set and the list of capabilities
that must not be lost.

## Stack

Vue 3 · Nuxt UI 4 · Pinia · `@tanstack/vue-query` · `ofetch` · vue-router ·
Vite. Same stack as `nuts-customer-portal`, which is the point: consolidating
on one frontend stack is the reason the rebuild happened.

Nuxt UI brings its own theming, which replaced the project-owned oklch token
system the old tree carried. That is why it was a rebuild and not a port.

## Running it

Server, runner and dashboard together, with seeded demo data:

```sh
node ../scripts/dev-stack.mjs
```

That starts `croniq-server` on `:4230` with a demo runner attached, and this
dashboard on `:4231`. See the script's header for prerequisites and for the
environment variables that move the ports when something else on the machine
holds one.

This tree alone, against a server you started yourself:

```sh
npm run dev        # :4231, proxies /v1 /health /version /metrics to :4230
```

The proxy is what makes development same-origin, so the `HttpOnly` refresh
cookie behaves exactly as it does in production
([ADR-0001](../docs/adr/0001-same-origin-dashboard.md)). There is deliberately
no `VITE_API_URL`: a cross-origin build downgrades refresh-token storage, and
rather than carry a build-time guard against that happening by accident, this
tree is same-origin only.

## Layout

```
app/
  api/          client (ofetch + Bearer + 401-refresh), session, types, queries
  lib/          framework-free helpers — SSE, the DSL renderer, the wasm bridge
  stores/       Pinia — auth, ui
  composables/  shared reactive behaviour
  components/   the pieces screens are built from
  pages/        routed screens
  router/       the route table
e2e/            Playwright smoke suite
scripts/        the checking tools below
```

`app/lib/sse.ts` came over unchanged: it is the tested SSE core from
[#585](https://github.com/nuetzliches/croniq/issues/585), the runners feed and
the log console both need it, and re-deriving it under a new reactivity model
is how a rebuild loses hard-won behaviour.

## Conventions

- **Import `ref`/`computed`/`watch` explicitly**, in `.ts` and `.vue` alike,
  even though Nuxt UI's `autoImport` supplies them at runtime. They are
  declared in `auto-imports.d.ts`, which the Vite plugin generates and which is
  gitignored and outside `tsconfig.app.json`'s `include` — so relying on them
  would make the type check depend on a build having run first, a footgun in CI
  and in a fresh clone. Nuxt UI's *components* (`UButton`, `UCard`, …) need no
  import: the package types them globally, so they work without the generated
  file.
- **Accessible names are not optional.** Every form control gets one as it is
  written; the ARIA containers are valid. This is the acceptance criterion
  [#595](https://github.com/nuetzliches/croniq/issues/595) was retargeted into,
  after the old tree turned out to have 51 of 54 inputs with no name at all.
  `scripts/accessible-names.mjs` checks it against Chromium's own tree.
- **Type-check with `npm run typecheck`, never `npx vue-tsc --noEmit`.** This
  tsconfig is a solution file (`"files": []` plus references), so `--noEmit`
  checks precisely nothing and reports success. Only `--build` descends into
  the projects that hold the code.

## Scripts

| | |
|---|---|
| `npm run dev` | dev server on :4231 |
| `npm run build` | `vue-tsc --build && vite build` — the type check is part of it |
| `npm run typecheck` | type check alone |
| `npm run lint` | eslint |
| `npm test` | vitest |
| `npm run test:e2e` | Playwright; brings its own stack up (`scripts/e2e-stack.mjs`) |

`predev` and `prebuild` build the WASM bridge into `app/lib/wasm/` via
`scripts/build-wasm.mjs`, which takes the destination as an argument.

### Checking tools

Not tests and not in CI — they need a dev stack, and each exists because some
claim about the dashboard was otherwise unfalsifiable. Run them from here with
`node ../scripts/dev-stack.mjs` up.

| | |
|---|---|
| `scripts/accessible-names.mjs` | every control's name, read from Chromium's AX tree over CDP |
| `scripts/write-paths.mjs` | drives every create/edit/delete path and reports 4xx or page errors |
| `scripts/dsl-parses.mjs` | feeds the rendered DSL back through the parser |
| `scripts/measure-scroll.mjs` | which elements actually scroll, and whether the page itself does |
| `scripts/capture-screens.mjs` | a screenshot of every screen, signed in, on real data |

## Dependency notes

**`overrides.fontless.esbuild`** in `package.json` forces esbuild to `^0.28.2`
*for fontless only*. [GHSA-g7r4-m6w7-qqqr](https://github.com/advisories/GHSA-g7r4-m6w7-qqqr)
(arbitrary file read from esbuild's *own* dev server on Windows) is patched in
0.28.1, but `fontless` — which reaches here as `@nuxt/ui` → `@nuxt/fonts` →
`fontless` — declares `esbuild: ^0.27.0` as a hard dependency and pins the tree
to the vulnerable line.

It is scoped rather than top-level (#681). A bare `"esbuild": "^0.28.2"` applies
to *every* edge, vite's included — and an override replaces the spec rather than
intersecting with it, with no ERESOLVE to say so. So when vite moves to esbuild
0.29, a top-level override would quietly hand it 0.28 and the breakage would
point at vite rather than at this line. The scoped form leaves every other
package resolving normally; today they all still dedupe onto 0.28.2 anyway.

The exposure is low — nothing here runs esbuild's serve mode; Vite's dev server
is Vite's own. It is overridden anyway because a standing advisory that is
always answered with "not our code path" trains you to skim the next one.

Remove the override once `@nuxt/fonts` ships a `fontless` on esbuild 0.28: the
check is `npm ls esbuild --all` after deleting it, and `npm audit` must stay at
zero. Being scoped, it can be deleted outright rather than narrowed first. The range is `^0.28.2` rather than `>=0.28.2` on purpose — esbuild's 0.x
minors carry breaking changes, and an open-ended floor would let one through
silently.
