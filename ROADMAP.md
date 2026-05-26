# Croniq Roadmap

Living punchlist of known improvements. Each item is sized for a single focused PR.
Last reviewed: 2026-05-26.

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

- **OTLP metrics push** — the OTLP exporter for traces + logs landed in #121
  ([README — Observability](README.md#observability),
  [crates/croniq-server/src/telemetry.rs](crates/croniq-server/src/telemetry.rs)).
  An OTLP push path for metrics is a parallel decision — some operators prefer
  push (collector-driven aggregation), some pull (Prometheus scrape). Keep
  `/metrics` as the default and add `otlp-metrics` as a follow-up Cargo feature
  once the job-level metrics above stabilise.

- **Trace propagation runner ↔ server** — the server now stamps a W3C
  `traceparent` into `WorkAssignment.metadata` (#172), but runners don't yet
  consume it: `croniq-runner-sdk` forwards the metadata opaquely and
  `croniq-shell-runner` never extracts it, so a job span still ends at the
  server's enqueue boundary. Have runners read the `traceparent` from
  execution metadata and continue the trace into the handler/subprocess
  (export `TRACEPARENT` for shell-runner subprocesses; attach a remote parent
  context in the SDK OpenTelemetry observers). Closes the runner half of #172.
  ([crates/croniq-runner-sdk/src/handler.rs](crates/croniq-runner-sdk/src/handler.rs))

- **OTel semantic conventions** — align span attribute names with the
  stabilising `messaging.*` / `cron.*` OTel semantic conventions once those
  hit Stable. Currently spans use Croniq-native attribute names
  (`job_key`, `execution_id`, `runner_id`).

## Tags follow-ups

Tags shipped for jobs (DSL `tags "k=v" …`) and runners (`RUNNER_TAGS` env)
as filter-only metadata distinct from routing-relevant capabilities. These
are the deliberate gaps left for follow-up:

- **Admin override for runner tags via API** — runners are the source of
  truth via self-registration on every poll, but ops sometimes wants to
  override (e.g. mark a runner `quarantine=true` from the UI without
  deploying). Add `PATCH /v1/runners/{id}/tags` with merge semantics:
  admin tags layer on top of the runner-declared set, persist across
  restarts in a small `runner_tag_overrides` table, and the registry
  unions them on each poll. ([crates/croniq-runner/src/registry.rs](crates/croniq-runner/src/registry.rs))
- **Tag injection into log events** — `ExecutionContext::push_log_events`
  already auto-injects `job_key` into every event's `fields`. Extend this
  to inject the job's tags + the runner's tags so log queries can filter
  by `tag:env=prod` without the call site having to thread the values
  through. Cheap on the SDK side; Loki/CloudWatch users get
  free filterable structured logs.
  ([crates/croniq-runner-sdk/src/handler.rs](crates/croniq-runner-sdk/src/handler.rs))
- **URL-state for list filters** — Jobs/Runners/Executions pages filter
  client-side; the state (selected tags, status, job-key substring) is
  not in the URL, so links/bookmarks don't capture context. Lift filter
  state into `useSearchParams` (`?tag=env=prod&tag=team=ops&state=failed`)
  and add a `?selected=<id>` param so deep-linking opens a specific
  detail panel. Would also collapse the nested-Sheet UX from the runner
  detail (clicking an execution there → navigates to
  `/jobs/<key>?execution=<id>` instead of stacking sheets).
  ([ui/src/pages/JobsPage.tsx](ui/src/pages/JobsPage.tsx),
  [ui/src/pages/RunnersPage.tsx](ui/src/pages/RunnersPage.tsx),
  [ui/src/pages/ExecutionsPage.tsx](ui/src/pages/ExecutionsPage.tsx))

## Tags hardening

- **Test coverage for tag plumbing** — the tag feature shipped without
  dedicated tests. Add: a parser-level test that the DSL `tags
  "env=prod" "team=ops"` directive populates `JobConfig.tags`; an
  axum integration test for `GET /v1/tags?entity={jobs|runners}` that
  asserts count aggregation + sort order; a UI test that filter chips
  apply AND-semantics. None block the feature today (manual smoke
  passed), but they're the kind of regression that's annoying to chase
  later.
  ([crates/croniq-config/src/compile.rs](crates/croniq-config/src/compile.rs),
  [crates/croniq-server/src/api/tags.rs](crates/croniq-server/src/api/tags.rs))
- **Tag validation rules** — tags are currently free-form strings with
  only "trim + dedupe + non-empty" enforced. Decide a policy:
  max length per tag, max tags per entity, forbidden characters
  (newline, control codes), case-insensitive dedup? The risk is mostly
  cosmetic (UI overflow, accidental `env=PROD` vs `env=prod` split)
  but worth pinning down before it ossifies.
  ([crates/croniq-server/src/api/jobs.rs](crates/croniq-server/src/api/jobs.rs),
  [crates/croniq-config/src/compile.rs](crates/croniq-config/src/compile.rs))

## Operator tooling

- **Add-Runner SDK template generator** — Runners page has no
  "Add Runner" affordance; new runners require reading the SDK README,
  finding the right `docker run` snippet, and minting an API key by
  hand. v1 scope: a wizard that asks {SDK target, capabilities,
  environment} and emits a ready-to-paste docker-compose snippet plus
  a freshly-minted scoped API key. v2 scope: full code-skeleton
  generation per language (Rust / Python / Shell-runner). Issue #93
  Wish 2. ([ui/src/pages/RunnersPage.tsx](ui/src/pages/RunnersPage.tsx))
