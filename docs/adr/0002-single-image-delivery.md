# ADR-0002: One image serves the API, the dashboard and MCP

- **Status:** Accepted
- **Date:** 2026-04 (recorded 2026-09-09)
- **Related:** [ADR-0001](0001-same-origin-dashboard.md), `docs/ui-architecture-decision.md`

## Context

Croniq is self-hosted software. The first thing a prospective operator does is
try to run it, and whatever that takes is the adoption cost. `docker compose
up`, then a browser on `:4000`, is the whole quickstart today — the demo
profile even seeds an admin, an API key and two runners so the dashboard has
data on first load.

The dashboard is a static Vite bundle. `croniq-server` takes a `--ui-dir` and
serves it with `ServeDir` plus an `index.html` fallback, on the same listener
as the API and the MCP transport. The published image builds the bundle in a
Node stage and copies `ui/dist` into the runtime image.

## Decision

The official `ghcr.io/nuetzliches/croniq` image contains the server binaries
*and* the dashboard, and `croniq-server` serves both on one port. This is the
supported default for quickstart, demo and single-host deployments.

## Alternatives considered

- **A separate UI image from the start.** Rejected in 2026-04 on two grounds:
  it makes the quickstart three services instead of one, and — the harder one —
  a dashboard on its own origin cannot receive the refresh cookie
  ([ADR-0001](0001-same-origin-dashboard.md)) unless a reverse proxy unifies
  the origin, which would make that proxy a mandatory part of every
  deployment.
- **Ship the dashboard in the release tarballs alongside the binaries.**
  Not done; the release artefacts carry binaries only, so the dashboard is in
  practice a Docker-only surface today. Revisit if binary installs need it.
- **Serve the dashboard from a CDN.** Same origin problem, plus it puts a
  network dependency in front of an on-premise scheduler's own admin UI.

## Consequences

- **UI↔API version skew cannot happen.** Both come from one digest.
- **Any UI change rebuilds everything.** A CSS fix goes through the Rust
  release build, `wasm-pack`, and the npm install, per platform.
- **The npm dependency tree is part of the build that produces the server
  binary.** Two supply chains share one build context.
- **Static-serving quality is limited to what `ServeDir` does.** It sets no
  `Cache-Control` and there is no compression layer, so every dashboard load
  revalidates every asset. That is a gap to close, not a consequence anyone
  chose — see the tracking issue.
- The dashboard cannot be scaled or cached independently of the scheduler.

## Enforced by

- `Dockerfile` — the `ui-builder` stage and the `COPY --from=ui-builder /build/ui/dist /usr/share/croniq/ui`
- `crates/croniq-server/src/main.rs` — `--ui-dir`, `ServeDir` + `ServeFile` fallback
- `docker-compose.yml` — the single-service quickstart
- `README.md` — the quickstart instructions

## What this ADR does not say

It does not say the dashboard may only ever ship inside this image. Publishing
an *additional* `croniq-ui` image for deployments that already run a reverse
proxy is compatible with this decision, as long as the combined image stays the
default quickstart and `--ui-dir` keeps working. It would need its own answer
for version skew, which this decision gets for free.
