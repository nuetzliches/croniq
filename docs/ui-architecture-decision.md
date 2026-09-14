# Trade-offs: UI container split + Vue rebuild

As of 2026-09-09. Basis: analysis of the current state of `ui/`, `Dockerfile`, `crates/croniq-server`,
`docs/operations.md`, `docs/vue-migration-plan.md` (2026-06-11) and the reference
projects `nuts-customer-portal/src/CustomerPortal.App` and `ciphr/ui`.

These are **two independent decisions**. They can be taken in any order — but the order
*split first, framework second* is markedly cheaper, because the split reduces the cutover
of a framework change to "swap the image tag".

---

## Current state (measured)

| Metric | Value |
|---|---|
| `ui/src` | 93 files, 16,364 LOC (14,076 TS/TSX + 2,288 CSS) |
| Pages | 10 + 3 settings tabs |
| Biggest chunks | `JobsPage.tsx` 1,478, `LoginPage.tsx` 1,096, `hooks.ts` 832, `AlertsPage.tsx` 785 |
| UI tests | 0 component/E2E tests (only 5 unit-test files on `lib`/`api`/`auth` helpers) |
| UI churn | 35 commits in 90 days ≈ 2.7/week (the June plan assumed 6/week) |
| Delivery | One image; `croniq-server --ui-dir /usr/share/croniq/ui` (`ServeDir` + index.html fallback) |
| Release binaries | contain **no** UI — in practice the dashboard exists only via Docker today |
| Coupling to Rust | `croniq-config-wasm` (DSL preview) is built with `wasm-pack` and copied into `ui/src/lib/wasm/` |

Since the June plan the UI has grown by ~42% (9,900 → 14,076 LOC TSX). The effort
named there is correspondingly no longer valid.

---

## Decision A — pull the UI out of the core image

### Pro

- **Decoupled release cadence.** A UI fix today forces the complete multi-stage build
  (Rust release per platform, wasm-pack, npm) and a new server image. Separated: a
  small Node-to-nginx image, seconds instead of minutes, and the server keeps its digest.
- **Smaller supply chain in the server image build.** The npm tree is today part of the
  build that also produces the server binary. Separate images = separate build contexts
  (`ciphr/ui` runs it explicitly that way, including `npm ci --ignore-scripts` + a license gate).
- **Real static-serving quality.** `ServeDir` today sets **no `Cache-Control`** and there is
  **no compression** — every dashboard load revalidates every asset. nginx/Caddy deliver
  `immutable` for `/assets/*`, `no-cache` for `index.html`, gzip/brotli and their own,
  stricter CSP for the static part at no extra effort.
- **Fits future operations.** Both references run exactly this pattern, and nuts-infra
  has no stack for croniq yet — so the split can be introduced *before* a
  deployment contract exists that would later have to be broken.
- **Scaling/CDN** becomes possible at all.

### Contra

- **Same-origin is a hard contract, not a detail.** Since #454 the refresh token lives in
  an `HttpOnly; SameSite=Strict; Path=/v1/auth` cookie. A UI on a different origin cannot get
  the cookie — `ui/vite.config.ts` therefore deliberately refuses the build when
  `VITE_API_URL` is set without `VITE_ALLOW_LOCALSTORAGE_REFRESH=1`. A split **without**
  a reverse proxy in front is therefore a deliberate security regression.
  *Resolvable:* a proxy in front (`/` → UI, `/v1` → server) — exactly the pattern of `ciphr/nginx.conf`
  ("no `proxy_pass` in the UI container, the proxy in front makes the origin") and
  `CustomerPortal.App/Caddyfile`. Then everything stays as it is.
- **The quickstart gets more expensive.** `docker compose up` → one server + demo runner, browser on
  `:4000`. After the split: a UI container + a proxy on top. For a self-hosted
  OSS product the one-container story is an adoption argument you should not give away.
- **Version skew becomes real.** Today UI/API is atomic. Separated, UI 0.39 can run against server 0.38.
  Needs at minimum: the same tag for both images as a documented rule, plus a
  startup check against `GET /version`.
- **Two delivery paths instead of one.** `--ui-dir` has to stay (air-gapped,
  binary installs, single VM), so the path is maintained anyway.
- **The WASM bridge stays attached to the Rust workspace.** A UI image still needs `wasm-pack` +
  `crates/croniq-config-wasm`. A separate **repo** would therefore be expensive; a separate
  **image from the same repo** costs almost nothing.
- Part of the caching benefit is available without the split too (`SetResponseHeaderLayer` +
  `CompressionLayer` in front of `ServeDir`, ~30 lines).

### Recommendation A: **Yes — additive, not as a replacement.**

1. A second image `croniq-ui` as **another target in the existing Dockerfile**
   (`FROM nginxinc/nginx-unprivileged AS ui-runtime`), built from the same `ui-builder` stage.
   Cost: one stage + one CI matrix entry. No second repo.
2. **Adopt the `ciphr/ui` pattern:** the UI container proxies *nothing*, it only serves files.
   The proxy in front makes the origin. That keeps the `HttpOnly` refresh cookie intact
   and `connect-src 'self'` valid.
3. **`--ui-dir` and the combined image stay the default** for quickstart/demo/single VM.
   The split is the deployment option for nuts-infra and the like, not the new obligation.
4. Immediately and independently of that: retrofit `Cache-Control` + compression for `ServeDir` —
   that is simply a gap in the delivery path today and affects every existing operator.
5. Document the rule: **both images carry the same tag**; the UI checks `/version` at startup
   and shows a skew notice instead of diverging silently.

---

## Decision B — rebuild with Vue 3

### Pro

- **Stack consolidation.** `nuts-customer-portal` and `ciphr/ui` are Vue; croniq is the
  outlier. For a one-person operation the context-switching costs are real and permanent.
- **Concrete React pain points disappear**, not only in the abstract: the
  `HTMLInputElement` prototype setter hack in `TimezoneInput` (only for react-hook-form),
  the "a hook returns JSX" confirm pattern across 8+ call sites, `useEffect` cancelled flags around
  the WASM preview. v-model/Teleport/`watch` replace that with nothing at all.
- **An opportunity to clear accumulated debt:** `recharts` is still in `package.json`
  *and* in the chunk rules, but has **zero imports**; `components.css` defines `.grid`,
  `.gap-*`, `.grow` unlayered and thereby overrides Tailwind utilities; two hand-built
  SSE clients (`hooks.ts`, `ConsolePage.tsx`); no UI tests.
- A somewhat smaller runtime (Vue 3 vs. React 19 + Radix) — nice, but not an argument.

### Contra

- **Cost without end-user benefit.** The June plan estimated 40–48 PD at 9,900 LOC TSX.
  At today's 14,076 LOC, **55–70 PD** is realistic (~10–14 calendar weeks on the side).
  No feature, no bug fix, no user notices it.
- **The safety net is missing entirely.** Without UI tests a 14k-LOC framework change is
  flying blind; a Playwright smoke suite (5–7 PD) is a precondition. But those 5–7 PD are
  valuable *independently of Vue* — the React UI lacks them just as much today.
- **Named risk clusters** from the June plan hold unchanged: the vue-query reactivity trap
  (parameters must be `computed`, otherwise filters freeze), the ConsolePage buffer
  (2,000 events ⇒ `shallowRef`, otherwise deep reactivity per SSE event), the auth redirect
  (Vue guards only fire on navigation ⇒ an additional `watch` is needed), CSS selectors on
  `data-` attributes break silently.
- **No technical forcing function.** React 19 + Vite 8 + Tailwind 4 is a current, maintained base.
- **The two references contradict each other** and the choice decides half the effort:

  | | `nuts-customer-portal` | `ciphr/ui` |
  |---|---|---|
  | Approach | Nuxt UI 4 + Pinia + vue-query + valibot + PWA | plain Vue 3, **zero** runtime deps besides `vue` |
  | Own code | little, the library carries the design | everything hand-written, including the router |
  | For croniq | replaces the project's own oklch token system with Nuxt UI theming ⇒ **redesign, not port** | too ascetic for 10 data-rich pages with builders and SSE |

### Recommendation B: **Not now as a big bang. Three stages instead.**

1. **Immediately (5–8 PD, valuable without a Vue decision):** phase 0 of the existing plan —
   Playwright smoke (~12–15 scenarios), `recharts` + unused Radix deps out,
   CSS collisions into `@layer`, the SSE core into *one* tested `lib/sse.ts`,
   the token holder detached from the store. That reduces the later porting surface
   *and* makes today's UI more maintainable. On abort: no total loss.
2. **Then carry out decision A** (the split). Only then is a framework cutover trivially
   rollback-able: a new `croniq-ui` image, switch the proxy route, rollback = the old tag.
   Before that the cutover would be a Dockerfile COPY path in the server image — doable, but
   without a clean reversal in live operation.
3. **Start Vue only with a written strategic justification** (the June plan demands that
   itself under "To be decided before starting", point 1). If the justification is
   "one UI family across nuts/ciphr/croniq", it is legitimate — but then please
   **port, not redesign**: Reka UI (headless) + the existing oklch tokens, as recommended in
   the June plan. Nuxt UI 4 would be the way if croniq is *to get* a visual redesign
   anyway; that is a product decision, not a framework decision, and should
   not happen as a side effect of a migration.

**If only one of the two is done: decision A.** It costs 2–4 PD,
measurably improves operations and delivery quality and takes no option away.
Decision B costs twenty times as much and is purely inward-looking.

---

## Proposed order

| # | Step | Effort | Value independent of the rest? |
|---|---|---|---|
| 1 | `Cache-Control` + compression for `ServeDir` | 0.5 PD | yes |
| 2 | Dead deps out, CSS collisions fixed, SSE core unified | 2–3 PD | yes |
| 3 | Playwright smoke as a CI job (not required at first) | 3–5 PD | yes |
| 4 | `croniq-ui` image as a second Dockerfile target + proxy docs + tag/skew rule | 2–4 PD | yes |
| 5 | Vue port — only after explicit justification, re-price the plan (55–70 PD) | large | no |

Steps 1–4 together are ~8–12 PD and deliver the bulk of the benefit of both ideas.
