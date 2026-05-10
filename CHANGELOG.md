# Changelog

All notable changes to Croniq are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Auto-injected `runner_id` and `runner_tags` on every log event** —
  `ExecutionContext::push_log_events` already auto-injected `job_key`;
  now also injects which runner instance produced the event and that
  runner's self-declared tags as a JSON-array string. Loki / CloudWatch
  / OpenSearch users get free filterable structured logs without
  threading these values through every call site. Callers can still
  override either field by setting it explicitly on `WorkEvent.fields`.
- **Per-line execution logs with level filter and search** — the
  shell-runner used to push captured stdout and stderr as exactly two
  big `WorkEvent` blobs per execution, which collapsed into a single
  multi-KB DOM node in the Execution Detail Logs panel and was
  unreadable for chatty jobs (CVE scans, long test runs, …). Each
  line now becomes its own `WorkEvent`, gets its own row in the
  `execution_logs` table, and renders as a memo'd `<LogLine>`. The
  panel grows level-filter chips (`info` / `warn` / `error`) and a
  substring search input above the log box, both client-side so
  navigation between filters is instant. Per-execution `seq` column
  (migration `010_execution_log_seq`) keeps order stable when many
  events share a millisecond timestamp; existing rows get `seq = 0`
  so old single-blob entries still display correctly. Server-side
  `ExecutionLogStore::append_logs_batch` writes the whole stream in
  one transaction so a 3000-line job no longer takes 3000 lock + INSERT
  round-trips, and `GET /v1/executions/{id}/logs` accepts an optional
  `?level=` query and returns up to 10 000 rows
  ([#108](https://github.com/nuetzliches/croniq/issues/108)).

### Changed

- **Per-route code splitting in the UI** — every page (`Dashboard`,
  `Jobs`, `JobDetail`, `Runners`, `DeadLetters`, `Executions`,
  `Calendars`, `Settings`) is now lazy-loaded via `React.lazy` so
  the heavy chunks (notably `recharts` at 357 kB used only on the
  Dashboard) are fetched on demand. Initial app bundle dropped from
  159 kB → 19 kB; first navigation pulls only the chunks the active
  route needs. A Suspense boundary in `Layout` provides a single
  shared spinner fallback while the chunk loads. Vendor chunks
  (`react`, `router`, `query`, `charts`, `radix`, `icons`, `forms`)
  remain split via the existing `rolldownOptions.codeSplitting.groups`
  config.

### Fixed

- **Persistent runner identity across container recreates** — the
  `croniq-shell-runner` and `croniq-demo-runner` binaries now read or
  generate-and-persist their `runner_id` at
  `${CRONIQ_RUNNER_DATA_DIR:-/var/lib/croniq-runner}/runner-id` instead
  of deriving it from the volatile container hostname. Operators who
  mount a small persistent volume on this path keep the same runner
  identity across `docker compose up -d --force-recreate`, so the
  Runner Detail Sheet's "Jobs Handled" and "Recent Executions" panes
  no longer reset on every recreate. Setting `RUNNER_ID` explicitly
  still overrides everything (#103).
- **Dead-letter writes now happen atomically with the dead-state transition,
  and orphans are backfilled** — the completion processor previously made
  two separate non-transactional store calls (`UPDATE executions SET
  state='dead'` followed by `INSERT INTO dead_letters`) and swallowed
  errors with `let _ = ...`. Failures of the second call left
  `state='dead'` rows with no corresponding dead-letter, so the Dead
  Letters UI page stayed empty even when actionable failures existed.
  New `DeadLetterStore::complete_as_dead` runs both writes in one
  transaction; the new SQLite migration `009_backfill_dead_letters`
  populates the table for existing orphan rows on first start
  (`expires_at = NULL` so the purge sweeper leaves them alone).
  Errors are now logged at `tracing::error!` level (#104).
- **Dead-letter retention TTL is now actually enforced** — the
  watchdog tick (every 30s) calls `store.purge_expired(now)` so rows
  whose `expires_at` has passed are reaped. The function existed since
  v0.9 but was never called from the server's main loop, meaning
  `dead_letter { retention 14d }` was a documented setting with no
  effect (#104).

## [0.10.1] - 2026-05-09

### Security

- **Bump `rmcp` ~1.3 → ~1.5 — patches DNS-rebinding in the Streamable
  HTTP transport** ([GHSA-89vp-x53w-74fx](https://github.com/modelcontextprotocol/rust-sdk/security/advisories/GHSA-89vp-x53w-74fx),
  CVE-2026-42559, CVSS 8.8). Croniq mounts rmcp's HTTP transport at
  `/mcp` via [`croniq-server::mcp::mcp_router`](crates/croniq-server/src/mcp.rs);
  rmcp < 1.4 did not validate the `Host` header, so a malicious public
  page could DNS-rebind a name it controls to the victim's local IP and
  invoke any MCP tool — including mutation tools — with the user's
  privileges. rmcp ≥ 1.4 ships a loopback-only `Host` allowlist by
  default (`localhost`, `127.0.0.1`, `::1`) and rejects mismatched
  hosts with HTTP 403 (f72da47).

  **Operator note for non-loopback deployments:** `croniq-server`'s
  default `--listen :4000` binds to `0.0.0.0`. If you currently reach
  `/mcp` via a public hostname, you will start receiving HTTP 403 from
  rmcp until an explicit allowlist is configured. Workaround until a
  Croniqfile-level `mcp { allowed_hosts … }` directive lands: front
  the server with a reverse proxy that rewrites `Host` to `localhost`,
  or wrap [`croniq_mcp::streamable_http_service`](crates/croniq-mcp/src/lib.rs)
  with an explicit `with_allowed_hosts(...)` call.

### Added

- **`SECURITY.md` published security policy** — establishes a private
  disclosure channel via GitHub Private Vulnerability Reporting (with
  email fallback), declares supported-versions matrix, and an initial
  response SLA. Surfaces a "Security Policy" tab on the repo landing
  page so researchers have a clear path that isn't a public issue
  (e9648c3).

## [0.10.0] - 2026-05-09

### Added

- **Free-form tags on jobs** — DSL syntax `tags "env=prod" "team=ops"` on
  `job:` blocks; `tags` column on `job_definitions` (migration 008); UI
  chip-bar filter (AND-semantics, multi-select) plus inline pills on the
  Jobs list and Job detail page; tags editable in the Edit-Job dialog
  for store-managed jobs. Tags are deliberately distinct from runner
  capabilities — they do NOT influence routing, only display + filter,
  so a typo in a tag can never break job dispatch
  (a42ad76).
- **Free-form tags on runners** — runners self-declare tags via
  `RUNNER_TAGS=env=prod,team=ops` env var (`croniq-shell-runner` and
  `croniq-demo-runner`); tags travel in every PollRequest so the server
  tracks them as live state, not registration-time snapshot; tag filter
  chips and per-card pills on the Runners page; tags shown in the
  Runner Detail panel
  (a5a5ec7).
- **`GET /v1/tags?entity={jobs|runners}`** — distinct tag values across
  the entity kind with usage counts, sorted by count desc then
  alphabetically. Powers the UI filter bars
  (a42ad76, a5a5ec7).
- **Runner detail Sheet panel** — clicking a runner card opens a slide-in
  with identity (id, status, last poll, capabilities, tags), the jobs
  this runner has actually handled recently (derived from execution
  history — Croniq routes by capability matching, so `assigned_runner_id`
  is null for most jobs), and the 10 most recent executions with
  click-through to the Execution Detail
  ([#93](https://github.com/nuetzliches/croniq/issues/93), 3cb4538, 57e4b7f).
- **Clickable Recent Executions on the Job detail page** — opens the
  same Execution Detail Sheet used on the Executions tab. The detail
  component is now extracted to `ui/src/components/ExecutionDetail.tsx`
  and reused across three pages
  ([#94](https://github.com/nuetzliches/croniq/issues/94), 472b93a).
- **Shell-runner stdout/stderr now reach the Execution Detail Logs
  section** — `handle_job` calls `ctx.push_log_events()` after
  `exec::run()`, capturing stdout as `info` and stderr as `warn` events.
  Failures to push are non-fatal (warned in tracing) so a flaky server
  never breaks a job run
  ([#92](https://github.com/nuetzliches/croniq/issues/92), 6d44dd0).
- **`ExecutionContext::push_log_events` + `log` SDK helpers** — narrows
  the runner-SDK API: `client` is now `pub(crate)`, external callers
  can no longer mis-call poll/ack/renew. `push_log_events` auto-injects
  `job_key` into every event's `fields` so log queries stay filterable
  by job even when the raw message doesn't carry it
  (6d44dd0).

### Fixed

- **Login error message disambiguates unreachable backend from wrong
  password** — a bare "Login failed. Check your credentials." led
  operators to chase password issues when the server was simply down.
  TypeError from `fetch()` (production network/CORS) and 5xx from the
  Vite dev-proxy (when upstream is gone) now surface as
  "Cannot reach server. Check that the Croniq backend is running."
  (89bf62d).
- **Executions page state-filter dropdown was empty** — the `STATES`
  constant was lost when `ExecutionDetail` was extracted to a shared
  component. Re-added in the same fix that derives Runner detail's
  job list from execution history instead of `assigned_runner_id`
  (which is null for most Croniq jobs in practice)
  (57e4b7f).

### Changed

- Demo `docker-compose.yml` seeds `RUNNER_TAGS=env=demo,role=worker` on
  demo runners so the Runners filter chip bar shows real data on first
  start. Override with `RUNNER_TAGS=` (empty) to clear.

## [0.9.1] - 2026-05-08

### Fixed

- **`POST /v1/trigger` now propagates DSL job metadata** to the runner. The
  manual-trigger endpoint built `WorkItem.metadata` from the HTTP request
  body only, discarding the DSL-compiled `__runner_exec` payload — so every
  manually triggered (and every retry of any) `runner shell { … }` or
  `runner exec { … }` job in v0.9.0 failed with the misleading
  `metadata is missing the __runner_exec payload` error. The trigger handler
  now seeds the metadata from `state.dsl_jobs` and overlays the caller's
  values on top, applying the same combined map to both the persisted
  `Execution` row and the dispatched `WorkItem`
  ([#89](https://github.com/nuetzliches/croniq/issues/89),
  [#90](https://github.com/nuetzliches/croniq/pull/90)).

## [0.9.0] - 2026-05-03

### Added

- **Generic shell runner** — new `crates/croniq-shell-runner` crate and
  `croniq-shell-runner` binary, shipped in the same `ghcr.io/nuetzliches/croniq`
  image. Picks up jobs declared with the new `runner shell { command "…" }`
  or `runner exec { args … }` Croniqfile blocks and dispatches them to a
  local subprocess — no Rust required for "run this shell command on a
  schedule" use-cases.

  ```croniqfile
  job ops:db-dump {
    every day at 03:00
    runner shell {
      command "pg_dump -U app app > /backups/app-$(date +%F).sql"
      workdir /opt
      env { PGPASSWORD secret-stuff }
    }
    timeout 10m
  }
  ```

  See [`README.md`](README.md#generic-shell-runner) for trust-model details
  and the recommended capability-based runner-pool layout.

### DSL

- `runner` now accepts a qualifier: existing `runner { require X }` keeps its
  placement-constraint meaning, and `runner shell { … }` / `runner exec { … }`
  carry execution payload (command, argv, workdir, user, env). Both blocks may
  coexist in the same job. The compiled payload is shipped to runners via the
  internal `__runner_exec` metadata key.
- New validation diagnostics: `runner shell` requires `command`; `runner exec`
  requires `args`; `command`/`args` are mutually exclusive between the two
  modes; unknown qualifiers (e.g. `runner http`) error out with a hint.

### Fixed

- **First-run init**: `croniq init` now validates the `--api-key` prefix (and
  `--scopes`) **before** any DB writes, so a malformed `CRONIQ_INIT_API_KEY`
  no longer leaves behind a half-initialized database (admin user created, no
  API key persisted) that masked the failure on subsequent boots
  ([#84](https://github.com/nuetzliches/croniq/issues/84),
  [#85](https://github.com/nuetzliches/croniq/pull/85)).
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

### CI / Dependencies

- Bump `actions/download-artifact` 7.0.0 → 8.0.1
  ([#79](https://github.com/nuetzliches/croniq/pull/79)).
- Bump `actions/upload-pages-artifact` 3.0.1 → 5.0.0
  ([#80](https://github.com/nuetzliches/croniq/pull/80)).
- Bump `docker/setup-qemu-action` 3.7.0 → 4.0.0
  ([#81](https://github.com/nuetzliches/croniq/pull/81)).
- Bump `docker/login-action` 3.7.0 → 4.1.0
  ([#82](https://github.com/nuetzliches/croniq/pull/82)).
- Bump `actions/deploy-pages` 4.0.5 → 5.0.0
  ([#83](https://github.com/nuetzliches/croniq/pull/83)).

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
