# Changelog

All notable changes to Croniq are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0] - 2026-04-25

### Added

- Per-handler scope checks on every authenticated endpoint. Tokens must
  carry the matching scope (e.g. `jobs:write`, `dead-letters:write`,
  `runners:read`, `work:poll`) or the wildcard `admin` scope; missing
  scope returns 403. The scope catalog lives in `croniq_auth::Scope` —
  see the README's *Scopes* section for the full table. Auth-disabled
  mode (no `pull_api.auth` and no `CRONIQ_JWT_SECRET`) keeps working for
  local dev: the middleware injects a synthetic admin context so the
  per-handler checks pass through.
- `croniq init --scopes a,b,c` mints a default API client with a narrow
  scope set instead of the implicit `[admin]`. Demo + quickstart still
  default to admin so `docker compose up` is unchanged.
- SIGHUP signal triggers a Croniqfile reload without restarting the server
  (Unix only). Matches the long-standing `kill -HUP <pid>` daemon convention
  so `docker compose kill -s HUP croniq` picks up Croniqfile edits without
  disturbing lease-active executions.
- `POST /v1/admin/reload-config` endpoint re-reads the Croniqfile and
  reconciles the live scheduler. Supports `?dry_run=true` to validate and
  return a diff summary without applying. Requires the `admin` auth scope.
  Validation failures return `422` with `line` and `column` when available.
- `croniq_config_reload_total{result=...}` Prometheus counter with labels
  `success`, `validation_error`, and `apply_error`.
- Structured position info on parse errors: `LoadError::Parse` now carries
  optional `line` and `column` fields derived from the parser's source spans.
- `ExecutionStore::create_execution_and_advance_job_state` — new trait
  method that commits the execution row + the advanced `job_state`
  (`next_fire_at`, `fire_count`) in a single transaction. Used by the
  scheduler tick to close the duplicate-fire-on-restart race; see
  *Fixed* below.

### Changed

- **Scheduler tick is now atomic.** Persisting an execution and
  advancing the trigger's `next_fire_at` previously happened in two
  separate writes, so a crash between them left the execution in the DB
  while the trigger still pointed at the old fire time — on restart the
  trigger fired again and produced a duplicate. Both writes now commit
  in one transaction; if the persist fails the in-memory `mark_fired`
  advance is rolled back so the next tick re-attempts cleanly.
- **Scheduler queue-overflow check is now O(1) per trigger.** The tick
  used to call `peek_n(1000)` and filter on `job_key` to count queued
  executions per job — O(min(queue_len, 1000)) per trigger per second.
  `WorkQueue` now maintains a per-job counter HashMap that is updated
  in lockstep with `enqueue` / `dequeue` / `remove` / `drain`, and the
  scheduler reads it via a single `count_for_job` lookup. Side benefit:
  the previous 1000-item cap silently under-counted for jobs deeper in
  the queue — now correct for any depth.
- The `--watch` file-watcher reload path now preserves API-registered
  triggers through a Croniqfile swap. Previously these triggers were dropped
  on reload because the scheduler's in-memory trigger map was fully replaced
  without re-merging API-managed entries from the store.
- `SchedulerCommand` has a new `Reload { triggers, jobs, ack }` variant used
  by the admin endpoint to swap state atomically and await confirmation.
- `--data-dir` now falls back to `$CRONIQ_DATA_DIR` when not set explicitly,
  matching how the Docker entrypoint already resolves it. The `CMD` in the
  official image no longer hardcodes the path so `docker run -e
  CRONIQ_DATA_DIR=…` overrides apply consistently to first-run init *and*
  the running server.
- The release workflow rewrites the workspace `version` to match the pushed
  tag at build time, so `--version` output (and the MCP server's
  identification handshake) always reflects the released version without
  requiring a manual `Cargo.toml` bump per release.
- `parse_duration_secs` now returns `Result<u64, String>` instead of
  silently falling back to 120s on malformed input. The boot path
  bubbles a specific error so a typo in `pull_api.lease_ttl` fails
  loudly instead of becoming a 2-minute lease nobody asked for.

### Performance

- Migration `005_perf_indexes` adds covering indexes that the two hot
  query paths actually use. Measured on 50k executions:
  - `find_queued_executions` (scheduler restore on boot):
    `state-only index + TEMP B-TREE FOR ORDER BY` (~2.8 ms)
    → composite `(state, fire_at)` walked in order (~0.04 ms) — **~70× faster**
  - `GET /v1/executions ORDER BY created_at DESC LIMIT 50`:
    full `SCAN executions` (~16 ms)
    → walks `idx_executions_created_at` (~0.02 ms) — **~720× faster**

### Fixed

- `apiDelete()` in the React UI silently treated 4xx/5xx responses as
  success (it only branched on 401). React Query's mutation success
  path then invalidated the cache, leaving the user looking at a
  "deleted" item that still existed. It now throws on `!res.ok`,
  matching `apiFetch()`.
- `install.sh` was hard-failing on every existing release because v0.4.0
  shipped without `SHA256SUMS` (404 → `set -e` aborts) and the next
  release would have written wrong paths into the manifest anyway. The
  installer now treats a missing manifest as a soft warning, and
  `release.yml` writes basenames so `sha256sum --check` finds them.
- Dockerfile used `npm install --frozen-lockfile`, a yarn/pnpm flag that
  npm silently ignores. The lockfile was never enforced. Switched to
  `npm ci`.
- `croniq init --api-key` previously produced a client locked to the
  hardcoded `[admin]` scope; combined with the new scope enforcement,
  this made non-admin keys impossible to mint via the CLI. The new
  `--scopes` flag fixes that.

### Security

- Bumped `jsonwebtoken` from 9 to 10 (advisory GHSA-c9xv-9rwj-9whw —
  type confusion that could lead to authorization bypass) and pulled in
  the `rust_crypto` feature, which 10.x requires explicitly.
- Bumped transitive `rustls-webpki` to ≥0.103.13 (DoS via panic on
  malformed CRL BIT STRING).
- Bumped transitive `postcss` to ≥8.5.10 (XSS in CSS Stringify).

### Removed

- `Formula/croniq.rb` is no longer kept in this repo. The Homebrew formula
  lives in `nuetzliches/homebrew-tap` and is updated by the release
  workflow; the local copy was a stale template (`version "0.1.0"` and
  zero-`sha256` placeholders) that confused first-time contributors.

### CI

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  -- -D warnings`, and `npm run lint` are now enforced. Test-code
  warnings (`unused must_use`, `unit_arg`, stale imports) that had
  accumulated under the old non-`--all-targets` clippy invocation are
  cleaned up.
