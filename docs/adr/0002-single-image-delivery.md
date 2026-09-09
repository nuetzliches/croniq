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
- ~~**Any UI change rebuilds everything.** A CSS fix goes through the Rust
  release build, `wasm-pack`, and the npm install, per platform.~~
  **Corrected 2026-09-09 — this was wrong.** The buildx cache
  (`type=gha, mode=max, scope=docker-<platform>`) keeps the `rust-builder`
  stage across runs, and a change under `ui/` invalidates only `COPY ui/` and
  what follows it. Measured across six consecutive `main` builds: a UI-only
  change costs **72–99 s**, a change under `crates/` costs **433 s**, and a
  docs-only change 21 s. The claim was written from the Dockerfile's structure
  rather than from a build, and the structure does not decide this.
- **The npm dependency tree is part of the build that produces the server
  binary.** Two supply chains share one build context. This is the one cost of
  the combined image that measurement did not shrink, and it is the argument a
  separate server image would rest on — see
  [#598](https://github.com/nuetzliches/croniq/issues/598).
- **The dashboard is not what makes the image big.** Published amd64 image,
  compressed layers: 55.92 MB total, of which the base (26.92 MB) and its
  `apt` layer (4.28 MB) are 56%, the five binaries are 24.3 MB, and
  `ui/dist` is **0.37 MB — 0.66%**. Recorded because "split the UI out to
  slim the image" is the obvious next thought and the numbers do not support
  it; the levers are the base image and which binaries ship —
  [#599](https://github.com/nuetzliches/croniq/issues/599).
- **Static-serving quality is limited to what the server implements.** As
  recorded, `ServeDir` set no `Cache-Control` and there was no compression, so
  every dashboard load revalidated every asset. That was a gap rather than a
  consequence anyone chose, and it was closed in
  [#582](https://github.com/nuetzliches/croniq/issues/582) — see
  `croniq_server::ui_assets`. Noted here because it was part of the picture
  when this decision was weighed, not because the decision changed.
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

It also does not settle **what the `croniq` tag should contain in the long
run**. Shipping a UI-less server as `croniq` and moving the combined variant to
a different name is a breaking change for every existing `docker pull`, and
that trade — a cleaner default against an established tag — is deliberately
left open here rather than decided in passing. It belongs with the work on the
image variants ([#587](https://github.com/nuetzliches/croniq/issues/587),
[#598](https://github.com/nuetzliches/croniq/issues/598)), not to this ADR,
which records only what ships today.
