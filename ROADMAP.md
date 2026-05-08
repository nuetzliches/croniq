# Croniq Roadmap

Living punchlist of known improvements. Each item is sized for a single focused PR.
Last reviewed: 2026-05-03.

## Security hardening

- **Verify checksums in `install.sh`** — publish `SHA256SUMS` alongside release
  tarballs and verify before extraction. Optional: minisign/cosign signatures.
  ([install.sh](install.sh))
- **Non-root Docker image** — add a dedicated `croniq` user in the Dockerfile;
  chown `/var/lib/croniq` and drop privileges via `USER`. ([Dockerfile](Dockerfile))
- **Persist the JWT secret** — server currently regenerates a random secret on
  every restart when none is configured, invalidating all runner tokens. Persist
  an auto-generated secret to `$DATA_DIR/jwt.secret` (mode 0600) on first boot.
  ([crates/croniq-server/src/main.rs:100](crates/croniq-server/src/main.rs))
- **Reject `admin/admin` outside demo mode** — the quickstart compose file sets
  a fixed admin password; refuse to start with that password unless
  `CRONIQ_DEMO_MODE=1` is explicitly set, or force a password change on first
  login. ([docker-compose.yml](docker-compose.yml))
- **Safer arg handling in entrypoint** — build an argv array in
  `docker-entrypoint.sh` so admin passwords containing spaces or shell-special
  characters survive. ([docker-entrypoint.sh](docker-entrypoint.sh))

## Release & CI hygiene

- **Pin GitHub Actions to commit SHAs** — enable Dependabot for the
  `github-actions` ecosystem so `actions/checkout`, `docker/*`, etc. get pinned
  and auto-updated.
- **Add `concurrency` blocks** — prevent overlapping CI runs and racing GHCR
  release pushes. ([.github/workflows/ci.yml](.github/workflows/ci.yml),
  [.github/workflows/release.yml](.github/workflows/release.yml))
- **Multi-arch Docker image** — release builds ARM64 tarballs but the Docker
  image is amd64-only. Add `platforms: linux/amd64,linux/arm64` to the
  `docker/build-push-action` call.
  ([.github/workflows/release.yml](.github/workflows/release.yml))
- **Explicit workflow-level `permissions: contents: read`** — belt-and-braces
  guard against future secret leakage.
- **Make `install.sh` resilient to GitHub rate limits** — use the
  `/releases/latest/download/…` redirect instead of parsing API JSON.

## UX loose ends

- **Client-creation scopes picker** — the Settings dialog currently hardcodes
  `['admin']` on every new client. Add a multi-select, default to `[]`, and let
  the server 400 if empty. ([ui/src/api/hooks.ts:220](ui/src/api/hooks.ts))
- **Calendar rules validation** — the `rules` field is a free-form textarea
  submitted without client-side parse feedback. Parse with the shared grammar
  before POST and surface 4xx errors inline.
  ([ui/src/pages/CalendarsPage.tsx](ui/src/pages/CalendarsPage.tsx))
- **Code-split large UI bundle** — the UI build warns about a >500 kB chunk;
  consider `manualChunks` in `vite.config.ts`.

## Observability

- **Job-level metrics** — the `/metrics` endpoint currently only exposes
  infrastructure-level gauges (runner count, queue depth). Add per-job metrics
  aggregated from the `executions` store:
  - `croniq_job_executions_total{job_key, status}` — counter, success/failure/timeout
  - `croniq_job_duration_seconds{job_key, quantile}` — histogram of `duration_ms`
  - `croniq_job_last_run_timestamp{job_key}` — gauge, Unix epoch of last fire
  - `croniq_job_log_bytes_total{job_key}` — counter, cumulative log volume pushed
    via `POST /v1/work/{id}/events`

  All four can be computed on-the-fly from the existing SQLite store on each
  `/metrics` scrape; no schema change required.
  ([crates/croniq-server/src/metrics.rs](crates/croniq-server/src/metrics.rs),
  [crates/croniq-store/src/traits.rs](crates/croniq-store/src/traits.rs))

## Integrations

- **`runner publish { … }` — hookaido bridge** — thin DSL block that
  publishes to a [hookaido](https://github.com/nuetzliches/hookaido) channel
  on fire instead of running a subprocess. Croniq stays the scheduler;
  hookaido handles durable HTTP delivery, HMAC signing, retries on the
  delivery side, and DLQ for failed callbacks. Shape (proposal):
  ```croniqfile
  job ops:nightly-poke {
    every day at 04:00
    runner publish {
      endpoint http://hookaido:8080/c/ops-nightly
      auth hmac {env.HOOKAIDO_INGRESS_SECRET}
      payload { window 24h; tag {vars.env} }
    }
  }
  ```
  Implementation lives next to `croniq-shell-runner`: a sibling
  `croniq-publish-runner` crate that consumes the same `__runner_*` metadata
  pattern. Until then, `runner shell { command "curl -X POST … " }` covers
  the use case.
