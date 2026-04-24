# Changelog

All notable changes to Croniq are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
