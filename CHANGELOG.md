# Changelog

All notable changes to Croniq are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Per-handler scope checks on every authenticated endpoint. Tokens must
  carry the matching scope (e.g. `jobs:write`, `dead-letters:write`,
  `runners:read`, `work:poll`) or the wildcard `admin` scope; missing
  scope returns 403. The scope catalog lives in `croniq_auth::Scope` —
  see the README's *Scopes* section for the full table. Auth-disabled
  mode (no `pull_api.auth` and no `CRONIQ_JWT_SECRET`) keeps working for
  local dev: the middleware injects a synthetic admin context so the
  per-handler checks pass through.
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

### Changed

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
