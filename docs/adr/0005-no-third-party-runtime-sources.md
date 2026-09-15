# ADR-0005: The dashboard loads nothing from a third-party origin

- **Status:** Accepted
- **Date:** 2026-09-15
- **Deciders:** Sebastian Gieseler
- **Related:** [ADR-0001](0001-same-origin-dashboard.md),
  [ADR-0002](0002-single-image-delivery.md),
  [ADR-0004](0004-vue-rebuild-for-the-dashboard.md)

## Context

Croniq is installed inside other people's networks. A large part of its
install base runs where Croniq is the only thing allowed to talk to the
internet, or where nothing is — air-gapped estates, regulated environments,
and the ordinary case of a scheduler on a private subnet. `docker compose up`
with one image is the supported shape (ADR-0002), and that shape promises the
image is the product: nothing is fetched at first paint.

The Vue dashboard broke that promise without anyone deciding to. Nuxt UI
renders icons through Iconify, whose default resolution path is a **runtime
HTTP request** to `api.iconify.design` (with `api.simplesvg.com` and
`api.unisvg.com` as fallbacks) for any icon not bundled at build time. Nuxt
UI bundles the icons *it* names in its own app config; the ~70 `i-lucide-*`
names this dashboard writes itself were not bundled, so every page load fired
a batch of cross-origin requests for them.

The consequences were already visible, in the order a user meets them:

1. The server's own `connect-src 'self'` (`api/hardening.rs`) blocked every
   one of those requests, so the icons simply did not render and the console
   filled with CSP violations. The hardening was right; the dashboard was
   wrong.
2. Where a CSP does not apply — a reverse proxy that strips the header, a
   `--ui-dir` served by someone else's web server — the requests *succeed*,
   and then every dashboard page view tells a third party that this
   deployment exists, when it is used, and by how many browsers.
3. On an air-gapped host they neither succeed nor fail fast. They hang on
   DNS, and the icons appear seconds late or not at all.

Any single one of these could be fixed where it appeared. All three come from
one missing constraint, and that constraint is not owned by any file: it lives
in the CSP, in the bundler config, in what `index.html` may reference, and in
what a future dependency is allowed to do at runtime.

## Decision

The dashboard resolves every resource it needs from its own origin. No script,
stylesheet, font, icon, image, or API request in the shipped bundle addresses a
host other than the one serving it. Assets come from the build; a dependency
that resolves assets over the network at runtime is configured to resolve them
at build time instead, and where it offers a build without the network path,
Croniq takes that build so the capability is not merely unused but absent.

## Alternatives considered

**Allow `api.iconify.design` in the CSP.** One line, and the icons come back
for everyone with internet access. Rejected: it fixes the symptom that was
loudest and leaves the two that matter more. Air-gapped installs still get
hanging loads, and every deployment starts reporting itself to a third party on
every page view — from a scheduler whose whole job is to run inside someone
else's perimeter.

**Self-host an Iconify API next to the server.** Iconify ships one, and it
keeps the on-demand model: new icons need no rebuild. Rejected as a second
service to run, configure and secure, contradicting ADR-0002's one image, in
exchange for a flexibility a dashboard with a fixed icon set does not need.

**Bundle the icons and stop there.** Configure the build to bundle the names
the source actually uses, leaving the API fallback in place for anything
missed. Rejected as the whole decision, kept as half of it: it is exactly the
state that produced this bug, because a fallback that works on the developer's
machine is a fallback nobody notices is being used. Bundling fixes today's
icons; it does not stop tomorrow's from being fetched.

**Vendor the SVGs into `public/icons/` by hand.** No build-time resolution to
get wrong, and the files are visible in the tree. Rejected: 70 files to keep in
step with the names in the source by hand, with no failure when they drift, and
it throws away Nuxt UI's icon component for a problem the bundler can solve.

## Consequences

- An icon this dashboard has never used cannot be added by writing its name.
  It must exist in an installed `@iconify-json/*` collection, and the build has
  to see the name in a file the scanner reads. A name assembled at runtime
  (`` `i-lucide-${kind}` ``) resolves to nothing, silently — the icon is
  simply absent. Icon names stay literal.
- The bundle carries every icon the source mentions, whether or not that page
  is ever opened. At ~70 icons this is a few kilobytes; at several hundred it
  would want revisiting.
- `@iconify/vue` is aliased to its `offline` build, which does not export
  everything the full one does. A Nuxt UI upgrade that reaches for a new export
  fails the build rather than the page. That is the intended trade — a loud
  failure at the only moment someone can weigh it — but it is a failure a
  routine dependency bump can hit.
- Croniq gives up on-demand icon resolution permanently, including the
  legitimate use for it: a future feature that lets an operator pick an icon
  for their own job or alert cannot offer the full Iconify catalogue. It gets
  the bundled set, or it gets its own ADR.

## Enforced by

- `crates/croniq-server/src/api/hardening.rs` — `CONTENT_SECURITY_POLICY`.
  `default-src 'self'` with no third-party source anywhere in it; the browser
  refuses what the build should never have emitted. Asserted by
  `crates/croniq-server/tests/http_hardening.rs`.
- `ui/vite.config.ts` — `icon.clientBundle.scan` makes the build resolve every
  `i-*` name in `app/` into the bundle, and `resolve.alias` points
  `@iconify/vue` at `app/lib/iconify-offline.ts`.
- `ui/app/lib/iconify-offline.ts` — re-exports `@iconify/vue/offline`, the
  build of Iconify that contains no API client at all. This is what makes the
  constraint structural rather than configured: there is no code in the bundle
  that could call out, so no configuration can be lost.
- `ui/scripts/check-local-sources.mjs` — run over `dist/` by `npm run
  check:sources` and by the `UI (build + typecheck)` CI job. Fails on any
  absolute URL in the built output that is not on the annotated allowlist, so a
  new dependency that phones home is caught at the PR, not in a customer's
  console.
- `ui/e2e/local-sources.spec.ts` — walks the signed-in dashboard with a route
  handler that fails the test on any request leaving the origin. The static
  check cannot see a URL assembled at runtime; this can.
- `ui/index.html` — no `<link>` or `<script>` to any other origin, and no
  font, analytics, or CDN tag. The entry document is where this is easiest to
  undo in one line.

## What this ADR does not say

Nothing here constrains what the **server** talks to. Croniq's own outbound
connections — alert webhooks, OTLP export, a Postgres store — are configured by
the operator and are the point of the feature. This ADR is about what a
*browser* is made to fetch by loading the dashboard, which the operator never
asked for and cannot see.

It also says nothing about the marketing site under `site/`, which is a
separate artefact with separate constraints.
