# Croniq UI — Vue rebuild

The dashboard being rebuilt in Vue 3, per
[ADR-0004](../docs/adr/0004-vue-rebuild-for-the-dashboard.md). **`../ui` is
still the dashboard that ships.** This tree replaces it only once it passes the
acceptance gate; until then both exist and both build in CI.

Read [`../docs/ui-screen-inventory.md`](../docs/ui-screen-inventory.md) before
adding a screen — it holds the agreed screen set, the build order, and the list
of capabilities that must not be lost.

## Stack

Vue 3 · Nuxt UI 4 · Pinia · `@tanstack/vue-query` · `ofetch` · vue-router ·
Vite. Same stack as `nuts-customer-portal`, which is the point: consolidating
on one frontend stack is the reason this rebuild exists.

Nuxt UI brings its own theming, which replaces the project-owned oklch token
system in `../ui/src/styles/`. That is why this is a rebuild and not a port.

## Running it

Both dashboards at once, against one server, so they can be compared:

```sh
node ../scripts/dev-stack.mjs
```

That starts `croniq-server` on `:4000` with seeded demo data and a runner, the
React tree on `:4231`, and this one on `:4232`. See the script's header for
prerequisites and for the environment variables that move the ports when
something else on the machine holds one.

This tree alone, against a server you started yourself:

```sh
npm run dev        # :4232, proxies /v1 /health /version /metrics to :4230
```

The proxy is what makes development same-origin, so the `HttpOnly` refresh
cookie behaves exactly as it does in production
([ADR-0001](../docs/adr/0001-same-origin-dashboard.md)). There is deliberately
no `VITE_API_URL` here — a cross-origin build downgrades refresh-token storage,
and the React tree needs a build-time guard to stop that happening by accident.
Rather than port the guard, this tree is same-origin only.

## Layout

```
app/
  api/        client (ofetch + Bearer + 401-refresh), session, types
  lib/        framework-free helpers carried over from the React tree
  stores/     Pinia — auth
  pages/      routed screens
  router/     route table, grown as screens land
```

`app/api/types.ts` and `app/lib/sse.ts` are carried over unchanged. They are
framework-free by construction, and `sse.ts` in particular is the tested SSE
core from [#585](https://github.com/nuetzliches/croniq/issues/585) — the
runners feed and the log console both need it, and re-deriving it under a new
reactivity model is how a rebuild loses hard-won behaviour.

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
  after the React tree turned out to have 51 of 54 inputs with no name at all.
- **The Playwright suite in `../ui/e2e` is the acceptance gate.** It asserts
  routes, session behaviour, URL contracts and both SSE surfaces — nothing
  about appearance — so it applies to this tree unchanged.

## Scripts

| | |
|---|---|
| `npm run dev` | dev server on :4232 |
| `npm run build` | `vue-tsc --build && vite build` — the type check is part of it |
| `npm run typecheck` | type check alone |
| `npm run lint` | eslint |
| `npm test` | vitest |

`predev` and `prebuild` build the WASM bridge into `app/lib/wasm/` via
`../ui/scripts/build-wasm.mjs`, which takes the destination as an argument so
one script serves both trees.
