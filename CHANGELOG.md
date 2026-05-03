# Changelog

All notable changes to Croniq are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **First-run init**: `croniq init` now validates the `--api-key` prefix (and
  `--scopes`) **before** any DB writes, so a malformed `CRONIQ_INIT_API_KEY`
  no longer leaves behind a half-initialized database (admin user created, no
  API key persisted) that masked the failure on subsequent boots
  ([#84](https://github.com/nuetzliches/croniq/issues/84)).
- **Docker entrypoint**: `docker-entrypoint.sh` now captures the exit status
  of `croniq init`, removes the partial `croniq.db` on failure, and exits
  non-zero — so the container crash-loops visibly instead of silently
  reporting healthy with broken auth. The error message also points the
  operator at the `croniq_` prefix requirement when applicable.

### Documentation

- README env-var table documents `CRONIQ_INIT_API_KEY`, including the
  `croniq_` prefix requirement and an `openssl rand -hex 32` example.
- `docker-compose.yml` clarifies the prefix requirement inline next to the
  demo `CRONIQ_INIT_API_KEY` value.

## [0.7.0] - 2026-04-27

### Added

- **HTTP MCP transport** mounted at `/mcp` on the same port as the REST API.
  Streamable-HTTP per the MCP spec, behind the same JWT/API-key auth layer.
  Toggle via Croniqfile `mcp { enabled false }`; default is enabled when the
  block is absent. Available alongside the existing stdio transport
  (`croniq-mcp`).
- `mcp:read` and `mcp:write` scopes for HTTP MCP access. `mcp:read` gates any
  `/mcp` request (initialize, tools/list, tools/call); `mcp:write`
  additionally gates the 17 mutation tools (`enqueue_job`, `cancel_execution`,
  `job_trigger`, all `*_job` / `*_schedule` / `*_calendar` mutations,
  `delete_runner`, `delete_dead_letter`, `dlq_retry`). `admin` is a wildcard.
- `croniq-mcp` toolset expanded from 12 to 31 tools — full CRUD over jobs,
  schedules, calendars, and dead letters; queue observability; live forecast
  (`dashboard_forecast`, HTTP transport only); execution log access
  (`get_execution_logs`).
- `PUT /v1/jobs/{job_key}` patches mutable job metadata (`description`,
  `timeout`, `max_retries`, `dead_letter_enabled`); JSON object semantics —
  missing key keeps the value, explicit `null` clears it. DSL-managed jobs
  return 409 `JobError::DslManaged`.
- `PUT /v1/calendars/{id}` patches name, timezone, and rules. Rules are
  re-validated through the Croniqfile DSL parser; failures return 422.
- `PUT /v1/api-clients/{id}` patches `name`, `scopes`, `is_active`. Empty
  scope set is rejected.
- Schedule calendar gating: `RegisterJobRequest` and `UpdateTriggerRequest`
  accept a `calendar` field (calendar name); `UPDATE schedules` matches
  `managed_by != 'dsl'` so DSL-owned rows are not mutated by API edits.
- `EditJobDialog` UI component with separate JOB and SCHEDULE fieldsets,
  Builder/Advanced toggle for the cron expression, `TimezoneInput`
  typeahead, and a clarified "Execution timeout" label.
- `CalendarPicker` dropdown reused in Create Job and Create/Edit Schedule,
  driven by `useCalendars()`.
- Pencil / "Edit" buttons on `JobsPage`, `CalendarsPage`, and `SettingsPage`
  rows; dialogs seed from the selected row and submit to the new PUT
  endpoints.
- API-key reveal in Settings is gated behind a confirm dialog with a
  "shown only once" warning.

### Changed

- Forecast logic moved from `croniq-server::dashboard` to
  `croniq-scheduler::forecast` so the HTTP API (`GET /v1/dashboard/forecast`)
  and the MCP `dashboard_forecast` tool produce identical bucketing from a
  single implementation.
- `DynStore` is now a re-export of `croniq-store::traits::Store`. The local
  `StoreExt` supertrait shim in `croniq-server::store` is removed.
- `humanizeScheduleError` (UI) accepts a `managedBy` context so a 409 on a
  non-DSL row renders the correct ownership message instead of a generic
  conflict toast.
- 409 errors on edit dialogs render inline next to the field instead of
  surfacing as a toast; the dialog stays open so the user can correct.

### CI/Build

- `actions/setup-node` bumped from a v4 SHA (Node 20 entrypoint) to v6.4.0,
  removing the "Node.js 20 is deprecated" warning that was being emitted on
  every CI run after the GitHub Actions Node 20 deprecation took effect.

## [0.6.0] - 2026-04-27

### Added

- `PUT /v1/schedules/{trigger_id}` patches API-managed trigger fields
  (`cron_expression`, `timezone`, `enabled`); DSL-managed rows return 409.
  The live scheduler is updated atomically via `Remove + AddJob`.
- Schedule create/edit dialog: pencil icon on each row seeds the form from
  the existing trigger; Builder/Advanced toggle for the cron expression,
  shared with the DSL generator.
- Dashboard stale-data banner: when the most recent execution is older than
  5 minutes, a banner appears above the Live Activity feed (30-second tick)
  with a link to `/runners` for investigation.
- Silence dividers in the activity feed between rows more than 30 minutes
  apart (`silence · 4h` instead of stacked timestamps).
- Standalone Croniqfile DSL generator at `/generator.html` with live
  `<output aria-live="polite">` and Copy button — matches the in-app
  Schedule and Calendar panels.
- WASM bridge for `croniq-config` via `wasm-pack`; the React UI calls
  `format_calendar_rules` and `format_schedule_inner` for canonical DSL
  preview output.
- Animated terminal demo on the landing page (typewriter effect, two tabs:
  `croniq quickstart` / `croniq jobs list`, autoplay on scroll, replay
  button).
- Install tabs on the landing page replacing the static install list:
  docker / brew / cargo / curl with copy buttons.
- Landing page footer with DSL Generator link, Changelog, contact email,
  Impressum, Datenschutz.
- "Trigger now" button on the JobDetail header next to "Copy job key";
  disabled when the job is inactive.

### Changed

- **Calendar rule editor rebuilt** in both surfaces (in-app
  `CalendarRuleBuilder` and `/generator.html`). Weekly: 7 day-toggle buttons
  plus Weekday / Weekend / Every-day presets. Monthly: 31 ordinal toggles
  plus "last day" plus 1st / 15th / Last presets. Annual: month `<select>`
  plus day `<input>` with live "Jan 25" preview. Window: two
  `<input type="time">` joined "to". Timezone: typeahead via
  `TimezoneInput` (in-app) / shared `<datalist>` (standalone).
- **Active-day DSL expansion**: parser accepts `Mon..Fri` ranges and
  `weekday` / `weekend` aliases; expands inclusively (wraps for long-weekend
  rotations like `Fri..Mon`). Weekdays are case-insensitive in 3-letter and
  full forms. The formatter emits 3-letter capitalised aliases
  (`weekday`, `Mon..Wed`) instead of quoted full names — output is stable
  round-trip.
- JobDetail header: relabeled `UPDATED` → `LOADED` for DSL-managed jobs
  (detected via `schedules.managed_by`); `Runner` → `Assigned Runner`.
- Timezone combobox is portaled to `<body>` so it floats above modal
  dialogs. Free-text validation: non-IANA values render with inline error
  and red border; empty stays valid (server falls back to UTC).
- `/generator.html` emits DSL into `<output aria-live="polite">` (was
  plain `<pre>`) for screen-reader consistency.

### Fixed

- **Weekday range expansion bug**: `Mon..Fri` previously dropped middle
  days, so calendars using ranges fired only on Monday and Friday instead of
  all five. Fixed by computing inclusive weekday ranges.
- **Docker build regression in the WASM bridge**: added a `wasm-builder`
  stage pinned to `$BUILDPLATFORM`, fetches a pre-built `wasm-pack`
  binary, copies the WASM artefacts into `ui/src/lib/wasm/` before
  `npm run build`. `build-wasm.sh` trusts existing artefacts in the
  Docker context so CI does not re-build inside the container.
- Timezone combobox: option clicks now properly select; the outside-click
  predicate now checks `listboxRef` so listbox clicks no longer dismiss.
- UI polish bundle: copy buttons, confirm dialogs on destructive actions,
  relative-time + ISO timestamps in tooltips, theme follows OS by default,
  pagination on long lists, dead-letter id surfaced in the table row.
- Responsive sidebar on narrow viewports; live polling on executions and
  dead-letter pages refreshes without manual reload.
- Scope picker sync (form state stayed stale on add/remove); fragment keys
  on list rendering; mutation toasts on success and failure paths; dialog
  labels matching their bound fields.
- UTF-8 string lexing in the Croniqfile parser; the schedule pluralisation
  rule now produces correct singular forms (`every 1 minute`, not
  `every 1 minutes`).
- Release workflow clones the tap repo for the formula update; the local
  `Formula/croniq.rb` was deleted in v0.5.0 so the previous in-place edit
  path no longer existed.

### Performance

- Multi-arch Docker build split across native runners: `ubuntu-latest`
  (amd64) and `ubuntu-24.04-arm` (arm64 native, no QEMU). Each platform
  caches independently; a `docker-merge` job stitches the manifests by
  digest into a single multi-arch list. Wall time ~10–15 min, down from
  ~60 min.
- Docs-only PRs skip the Docker build and release-binary jobs by detecting
  changes against `docs/`, `site/`, `README*`, and `CHANGELOG*`.

### Documentation

- `croniq-server --help` documents the `RUST_LOG` env var for
  level/per-module log control.

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
