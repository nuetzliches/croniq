# Changelog

All notable changes to Croniq are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.16.0] - 2026-05-26

### Added

- **Live Console: tail server tracing events in real time from the
  dashboard ([#141](https://github.com/nuetzliches/croniq/issues/141)).**
  New `/console` route in the UI streams every server-side tracing
  event (filtered by the operator's `RUST_LOG`) as soon as it's
  emitted. Level chips (debug/info/warn/error) toggle the SSE
  subscription server-side; a substring search runs locally over
  message/target/structured-field values. Sticky auto-scroll with
  scroll-lock, Pause/Resume (buffers without dropping), Clear, Copy,
  and Download `.ndjson` actions cover the common operator workflows.
  Backed by a new `GET /v1/events/stream` SSE endpoint and an
  in-process `ConsoleHub` (bounded 1000-entry ring buffer +
  `tokio::sync::broadcast` channel). Required scope:
  `executions:read`. Stream-open is audited as `console.opened`. The
  hub honours `RUST_LOG` via an explicit per-layer `EnvFilter` so a
  server started with `RUST_LOG=info` does not pump debug events into
  the dashboard. Persistent long-term log search is intentionally
  out of scope — use OTLP for that; the ring buffer exists to give a
  freshly opened dashboard ~1 min of backfill before the live tail.

- **Server routes admin-issued cancels to runners via `PollResponse.cancel`
  ([#176](https://github.com/nuetzliches/croniq/issues/176)).** Closes the
  long-standing gap where the wire contract had a `cancel` field but the
  server never populated it. Two-PR landing:
  - **Server side (PR-1 of 2,
    [#193](https://github.com/nuetzliches/croniq/pull/193)):** new
    `POST /v1/executions/{id}/cancel` endpoint. Queued executions
    flip directly to `cancelled` in the store; claimed (in-flight)
    executions are flipped AND pushed onto the owning runner's
    in-memory cancel queue, delivered on its next poll. Idempotent
    on already-cancelled (returns 200), 409 on terminal states
    (`completed`/`failed`/`dead`). New `executions:cancel` scope —
    granted to Operator by default; admin's wildcard covers it.
    Cancel issuance is audited as `execution.cancelled`. The
    Executions page in the dashboard gets a per-row Cancel button
    for `queued`/`claimed` rows.
  - **SDK side (PR-2 of 2,
    [#197](https://github.com/nuetzliches/croniq/pull/197)):**
    every SDK now keeps polling while at capacity, sending its
    current inflight list, so the server can deliver cancels via
    `PollResponse.cancel`. Before this, the SDKs' `sleep
    (capacity_backoff)` branch suppressed polling and a
    `max_inflight=1` runner could never receive a cancel until the
    handler finished naturally. The server's poll handler returns
    immediately on capacity=0 (no long-poll), so each at-capacity
    poll round-trips through `capacity_backoff` (default 500 ms in
    .NET/Go/Python/TS/Java; **now 500 ms in Rust too**, was 5 s in
    the original PR before review). New conformance case
    `04a-cancel-at-max-inflight-1.yaml` pins the behaviour for the
    five SDKs that act on cancels. **Caveat:** the Rust SDK polls
    correctly at capacity but **does not yet abort the handler
    future** on `PollResponse.cancel` — it surfaces the cancel as
    a `warn` log instead. Handler abort is a future enhancement;
    operators using cancels against a Rust runner will see
    `delivered_via_runner: true` from the server but the handler
    runs to completion.
  - **Drive-by Java SDK wire fix:** the Java SDK was sending
    `slotsFree` (max_inflight minus current inflight) AS
    `PollRequest.max_inflight`, which the server then double-
    discounted. A Java runner with `maxInflight=5` and 2 inflight
    was effectively running with 3 slots, not the configured 5.
    Fix sends the configured `options.maxInflight()` like the other
    SDKs — silent correctness restoration; no migration steps.

- **Inline stale-runner takeover replaces the 10-minute watchdog wait
  ([#190](https://github.com/nuetzliches/croniq/issues/190),
  [#192](https://github.com/nuetzliches/croniq/pull/192)).** When a
  runner restarts under the same `runner_id` with a fresh
  `instance_id` after its previous session went silent, the
  registry's instance guard used to reject every poll with `409
  Conflict` until the watchdog evicted the dead entry — observed
  delay in production: ~10 minutes. The registry now disambiguates a
  real conflict (two live processes racing) from a takeover (old
  session past `lease_ttl_secs`) and evicts the dead entry inline.
  Any executions still claimed-in-store by that `runner_id` are
  requeued by a spawned task so the poll returns promptly. New
  `RegisterOutcome` enum (`New` / `Updated` / `TookOver {
  previous_instance_id }`) on `register_or_update` replaces the
  previous `Result<bool, _>`. The watchdog's per-runner cleanup is
  shared via a new `requeue_abandoned_for_runner` helper so the
  inline takeover and the periodic sweep stay behaviourally
  identical. Watchdog also drops the dead runner's `cancel_queues`
  entry to prevent unbounded growth on long-lived servers.

### Changed

- **Runner SDKs (Rust + .NET) treat repeated `409 Conflict` on poll as
  fatal ([#134](https://github.com/nuetzliches/croniq/issues/134)
  sub-item 1).** Today the SDKs back off and retry forever on any poll
  failure — including the 409 the server returns when another process
  is already registered with the same `runner_id`. That masks an
  operator misconfiguration as a transient blip and ties up the runner
  process indefinitely. The SDKs now count *consecutive* 409s; after
  `max_consecutive_poll_conflicts` (default `3`) the run-loop exits
  with a typed error so the host process terminates with a non-zero
  status code instead of looping silently. The counter resets on any
  successful poll or non-409 transient error (5xx, network, timeout),
  so a single recovered 5xx doesn't accumulate against the conflict
  budget.

  Configuration knobs:

  | SDK | Knob | Default |
  |---|---|---|
  | Rust (`RunnerBuilder`) | `.poll_retry_delay(Duration)` (newly configurable; was hard-coded 5 s) + `.max_consecutive_poll_conflicts(u32)` | `5s` / `3` |
  | .NET (`CroniqRunnerOptions`) | existing `PollRetryDelay` + new `MaxConsecutivePollConflicts` (int, `[Range(1, 100)]`) | `5s` / `3` |

  Error surface:

  - Rust: new `ClientError::PollInstanceConflict { body }` variant returned from `runner.start()` after N conflicts.
  - .NET: new public `PollInstanceConflictException` (carries `RunnerId` + `ConsecutiveCount`) thrown out of `RunAsync`.

  The conformance schema gains a `max_consecutive_poll_conflicts`
  field on `runner_config` (and the .NET binding wires it through to
  `CroniqRunnerOptions`), so a future shared case can pin the wire
  contract once the Go, Python, and TypeScript SDKs implement the
  same behaviour. The Rust + .NET behaviour is verified at the
  SDK unit-test layer for now. Case 11 (single 409 → transient
  retry) stays unchanged.

- **CI: SDK build/conformance/smoke jobs short-circuit when nothing
  relevant changed ([#196](https://github.com/nuetzliches/croniq/pull/196)).**
  Each of the five SDK CI workflows (`dotnet`, `go`, `java`, `python`,
  `typescript`) gains a `changes` job that uses `dorny/paths-filter`
  to detect whether anything under `sdks/<lang>/**`,
  `sdks/conformance/**`, `openapi.yaml`, or its own workflow file
  was touched. Heavy build / conformance / pack-smoke jobs gate on
  the filter via `if: needs.changes.outputs.<lang> == 'true'` and
  skip otherwise. The required aggregator job (`<Lang> SDK CI
  required`) treats `skipped` as success because its existing
  `contains(needs.*.result, 'failure'|'cancelled')` check excludes
  `skipped`, so the branch-protection contract is preserved.
  Schema validation stays always-on (3 s job, catches typos in
  conformance YAMLs independently). Result: a typical server-only
  PR drops from ~35 long-matrix SDK jobs to ~5 short `changes` +
  5 aggregators — roughly 5+ CI-hours → ~3 minutes.

### Added

- **Server stamps W3C `traceparent` into `WorkAssignment.metadata`
  ([#134](https://github.com/nuetzliches/croniq/issues/134), sub-item
  4).** With the `otlp` feature on and an OTLP endpoint configured,
  the scheduler injects the active tick-span's `traceparent` (and
  `tracestate`, when present) into the metadata of every work
  assignment shipped to a runner. Runner SDKs that pass the metadata
  through their platform's W3C propagator (`Propagators.DefaultTextMapPropagator.Extract`
  in .NET, `opentelemetry.propagate` in Python, …) now produce
  execute-spans that hang off the server's fire-span instead of
  starting orphan traces. Without the `otlp` feature, or when no
  tracer provider is installed, the helper is a no-op — metadata is
  byte-identical to today.

### Fixed

- **Java SDK: root `build.gradle.kts` no longer references the
  removed `nexus-publish` plugin
  ([#195](https://github.com/nuetzliches/croniq/pull/195)).** The
  Maven-Central migration in
  [#188](https://github.com/nuetzliches/croniq/pull/188) switched the
  per-module publishing convention to the Vanniktech plugin but left
  a stale `alias(libs.plugins.nexus.publish)` + `nexusPublishing {…}`
  block in the root build script. The nexus alias was removed from
  `libs.versions.toml` in the same PR, so every Java SDK CI run
  failed at configuration time with `Unresolved reference: nexus`,
  which in turn blocked every PR (including pure-Rust changes)
  because Java SDK CI is a required check. Drive-by: the `otel`
  module's `publishing { publications.named<MavenPublication>("maven")
  }` (legacy OSSRH form) is replaced with Vanniktech's
  `mavenPublishing { coordinates(…) }` to fix the matching
  `Publication with name 'maven' not found` Spotless failure.

### Security

- **Dependabot: vite + esbuild in
  `sdks/conformance/bindings/typescript`
  ([#191](https://github.com/nuetzliches/croniq/pull/191)).** Vitest
  was pinned to `^2.1.9`, which pulled in vite 5.x (Path Traversal
  in Optimized Deps `.map` Handling) and esbuild <=0.24.2 (dev
  server CORS bug enabling cross-site requests). Bumped to vitest
  `^4.1.7`, matching the sibling `sdks/typescript` workspace — vite
  ^8 / esbuild ^0.27/0.28, both past the vulnerable ranges. Vitest
  is dev-only so no runtime behaviour changes.

### Documentation

- **`openapi.yaml`: `POST /v1/work/poll` documents `cancel` as
  reserved-but-empty
  ([#134](https://github.com/nuetzliches/croniq/issues/134) sub-item 2,
  follow-up tracked in
  [#176](https://github.com/nuetzliches/croniq/issues/176)).** Every
  SDK already honours a populated `cancel` array (proven by conformance
  case 04 with a mocked server response), but the server never
  populates it today — there's no admin endpoint, per-runner cancel
  queue, or routing into the poll response. The wire field stays
  reserved so SDK behaviour doesn't have to change when the server
  side lands; the endpoint description now spells out the current
  state and points operators at the `max_inflight >= 2` workaround
  for any runner that needs in-flight cancellation. **Superseded
  later in this release by [#193](https://github.com/nuetzliches/croniq/pull/193) +
  [#197](https://github.com/nuetzliches/croniq/pull/197):** the
  workaround note has been removed; the field is now actively
  routed.

## [0.15.0] - 2026-05-26

### Added

- **Failure alerts: rules + multi-channel dispatch supersede
  `CRONIQ_ON_FAILURE_CMD`
  ([#140](https://github.com/nuetzliches/croniq/issues/140)).** A new
  top-level `alerts { … }` block in the Croniqfile lets operators
  declare named channels and per-rule routing for permanent failures
  *and* SLA misses. Six PRs landed end-to-end:
  - **PR-1 foundation**
    ([#146](https://github.com/nuetzliches/croniq/pull/146)) — DSL
    parser, evaluator, shell channel, in-process `(rule, job_key)`
    throttle map, `alert_deliveries` table (migration 017), audit
    hook. The legacy `CRONIQ_ON_FAILURE_CMD` env-var is auto-
    synthesised as a single rule pointing at a shell channel, so
    existing operators upgrade with no Croniqfile changes.
  - **PR-2 webhook channel**
    ([#147](https://github.com/nuetzliches/croniq/pull/147)) —
    `webhook { url … sign hmac {env.SECRET} timeout 5s }` with
    HMAC-SHA256 signing (`X-Croniq-Signature` header) and one
    exponential-backoff retry on 5xx / network failure. The signing
    key is `#[serde(skip_serializing)]` so it never leaks via
    `/v1/alerts/config`.
  - **PR-3 email channel**
    ([#150](https://github.com/nuetzliches/croniq/pull/150)) —
    multi-recipient `email "ops@…" "oncall@…"` using the existing
    `EmailSender` trait, so the same SMTP transport feeds invitations,
    password resets, *and* alerts. `NoopSender` keeps working as the
    log-only fallback when SMTP isn't configured.
  - **PR-4 `job_sla_missed` trigger**
    ([#155](https://github.com/nuetzliches/croniq/pull/155)) — the
    watchdog sweeps in-flight executions every ~30 s; rules with
    `when job_sla_missed` + `expected_within 15m` fire once per
    `(rule, execution_id)` (dedup set prevents re-firing while the
    execution stays in-flight). Shares `dispatch_rule` with the
    failure path so `throttle 10m` applies across both trigger types
    on the same `(rule, job_key)`.
  - **PR-5 read-only API**
    ([#161](https://github.com/nuetzliches/croniq/pull/161)) —
    `GET /v1/alerts/config` (effective rules + channels, secrets
    stripped), `GET /v1/alerts/deliveries?job_key=…&state=…` (with
    `since`, `rule_name`, `limit ≤ 500`), and a single-row
    `GET /v1/alerts/deliveries/{id}` for the UI detail pane. New
    `alerts:read` scope.
  - **PR-6 operator UI**
    ([#163](https://github.com/nuetzliches/croniq/pull/163)) — new
    top-level `/alerts` page (Configuration + Recent deliveries tabs,
    15 s polling), sidebar entry, and a `job_key`-scoped slice of the
    delivery log under Jobs → Alerts.

  Migration: `017_alert_deliveries.sql`. See
  [`docs/operations.md`](docs/operations.md) for the directive
  reference and the legacy env-var fallback path.

- **Operators can disable password login via
  `auth.password.enabled`
  ([#138](https://github.com/nuetzliches/croniq/issues/138),
  [#144](https://github.com/nuetzliches/croniq/pull/144)).** New
  top-level DSL block:
  ```hcl
  auth {
    password { enabled false }
  }
  ```
  Env override: `CRONIQ_PASSWORD_LOGIN_ENABLED=false`. The server
  refuses to start when *both* password and OIDC are disabled, with a
  clear error pointing at the DSL block. When the flag is off,
  `POST /v1/auth/login`, `POST /v1/auth/login/totp`, and the
  password-reset endpoints all return `403 {"error": "password login
  disabled"}`. PAT minting (which only authenticated users can reach)
  is unaffected. New combined probe `GET /v1/auth/config` surfaces
  both auth-method gates (`oidc.enabled`, `password.enabled`) in a
  single response so the UI login page renders the correct flow
  without parsing JWT internals.

- **Public `GET /version` endpoint
  ([#135](https://github.com/nuetzliches/croniq/issues/135),
  [#136](https://github.com/nuetzliches/croniq/pull/136)).**
  Anonymous version probe — returns `{ version, git_sha, build_time,
  env }` for monitoring / orchestrator health checks. No auth, no
  user-controllable input, safe behind public load balancers.
  Complements the existing authenticated `GET /v1/version`.

- **Language-agnostic YAML conformance suite for runner SDKs.** New
  [`sdks/conformance/`](sdks/conformance) tree with shared
  `cases/*.yaml` test scenarios and a `schema/` JSON Schema that the
  CI pipeline validates on every push. Each SDK ships a small binding
  shim that loads the YAML and executes the cases against the
  conformance protocol. Currently covers register / poll / claim /
  ack / nack / streaming-log batching / graceful drain. Used by the
  .NET, Go, Python, and TypeScript SDKs to guarantee
  protocol-equivalence — see
  [`sdks/conformance/README.md`](sdks/conformance/README.md).

- **First-class .NET 8 + .NET 10 runner SDK
  ([#129](https://github.com/nuetzliches/croniq/pull/129)).**
  `Croniq.Runner.Sdk` (and the optional
  `Croniq.Runner.Sdk.OpenTelemetry` extension) ship as NuGet packages
  with `Microsoft.Extensions.Hosting` integration, bundled OTLP
  exporter, streaming-log forwarding (`LogWriter` with bounded
  backpressure + batch-by-32 / batch-by-200 ms), per-execution
  `CancellationToken` honouring server-driven cancellation, drain-
  before-ack on shutdown, and self-registration of schedule-bearing
  handlers. `ApiKey` / `Bearer` credential precedence matches the
  server contract. Source at [`sdks/dotnet/`](sdks/dotnet); first
  release tag is `dotnet-sdk-v0.1.0`.

- **Go runner SDK
  ([#131](https://github.com/nuetzliches/croniq/issues/131),
  [#149](https://github.com/nuetzliches/croniq/pull/149)).** Module
  `github.com/nuetzliches/croniq/sdks/go` with idiomatic
  `croniq.Run(ctx, opts, handler)` API, graceful drain on
  SIGINT/SIGTERM, structured logging, and an optional
  `sdks/go/otel` sub-module for OTLP trace + log export. Passes the
  full conformance suite. First release tag: `sdks/go/v0.1.0`
  (plus `sdks/go/otel/v0.1.0`).

- **Python runner SDK
  ([#130](https://github.com/nuetzliches/croniq/issues/130),
  [#158](https://github.com/nuetzliches/croniq/pull/158)).** Package
  `croniq-sdk` on PyPI, Python ≥ 3.11. Async-first
  (`croniq.run_runner(...)`) with sync handler bridging via a
  thread pool, streaming-log forwarding, graceful drain, and
  pluggable OTel propagation. Source at
  [`sdks/python/`](sdks/python); first release tag is
  `python-sdk-v0.1.0`.

- **Automated .NET SDK release pipeline.** New workflow
  `.github/workflows/dotnet-sdk-release.yml` triggers on
  `dotnet-sdk-v*` tags: restores → builds → runs unit + conformance
  tests → `dotnet pack` (MinVer reads the tag, sets the version on
  both `Croniq.Runner.Sdk` and `Croniq.Runner.Sdk.OpenTelemetry`) →
  `dotnet nuget push` to nuget.org (with `--skip-duplicate` so a
  re-run after a partial failure doesn't 409). Requires the
  `NUGET_API_KEY` repo secret. Symbol packages (`.snupkg`) push
  alongside the main `.nupkg`. Closes the gap between PR-time CI
  (which only `pack-smoke`d) and a real release.

- **Automated TypeScript SDK release pipeline.** New workflow
  `.github/workflows/typescript-sdk-release.yml` triggers on
  `ts-sdk-v*` tags: typecheck → lint → unit + conformance tests →
  build → `npm publish --provenance --access public` to npm.
  Pre-release tags (with `-`) ship under the `next` dist-tag so
  `npm install @nuetzliches/croniq-runner` keeps pointing at stable.
  Requires the `NPM_TOKEN` repo secret.

- **TypeScript / Node.js runner SDK (closes #132).** New package
  `@nuetzliches/croniq-runner` at [`sdks/typescript/`](sdks/typescript). ESM-only,
  Node ≥ 18, native `fetch` and `AbortController`. Ports the .NET SDK's
  semantics: poll / ack / renew / events / register loop, per-execution
  `AbortSignal` honouring `PollResponse.cancel`, streaming `LogWriter`
  with bounded backpressure, batch-by-count (32), batch-by-time (200 ms),
  drain-before-ack, self-registration of schedule-bearing handlers, and
  `ApiKey`/`Bearer` precedence. Passes all 12 cases in
  [`sdks/conformance/cases/`](sdks/conformance/cases) via a new TS
  binding at [`sdks/conformance/bindings/typescript/`](sdks/conformance/bindings/typescript).
  Dedicated CI workflow at `.github/workflows/typescript-sdk-ci.yml`
  (schema → build & test on Node 18/20/22 × Linux/macOS/Windows →
  conformance → pack-smoke → required aggregator) mirrors the .NET
  workflow's branch-protection contract.

- **OpenAPI 3.1 spec covers the PR-A1…B1b surface (PR-B1c).** Every
  new endpoint added between PR-A1 and PR-B1b is documented in
  `openapi.yaml` — User CRUD, Invitations, Password-Reset, TOTP,
  PATs, OIDC (login + callback + config), the MFA step-up exchange,
  Audit-Log read, per-job stats, throughput, failure heatmap. Schema
  components include `User`, `Role`, `Invitation`, `PersonalAccessToken`,
  `TotpSetupResponse`, `OidcConfigResponse`, `MfaRequiredResponse`,
  `AuditEvent`, `JobStatsResponse`, `ThroughputResponse`, and
  `FailureHeatmap`. SDK generators and external API clients can pick
  up the new surface mechanically.

- **UI login flow understands the MFA step-up + OIDC button (PR-B2).**
  `LoginPage` now drives the two-step exchange added in PR-A3: when
  `/v1/auth/login` returns `{requires_totp, mfa_token}` it switches
  to a 6-digit code prompt with an inline "Use recovery code"
  toggle. An "OIDC sign-in" button appears when
  `/v1/auth/oidc/config` reports the provider is configured;
  otherwise the SSO panel stays hidden so the page is unchanged for
  password-only deploys. New TypeScript types in
  `ui/src/api/types.ts` cover the full PR-A1…B1 surface (User,
  Invitation, PAT, TotpSetupResponse, AuditEvent, JobStatsResponse,
  ThroughputResponse, FailureHeatmap).

- **Audit log + per-job stats + throughput + failure heatmap (PR-B1).**
  Backend foundation for the redesigned Dashboard / Insights pages.
  Read-only aggregations, all computed on-the-fly from the existing
  executions table (no extra materialisation yet).
  - `GET /v1/audit` — list events with optional filters
    (`actor_type`, `actor_id`, `action`, `target_type`, `target_id`,
    `since`, `until`, `limit ≤ 1000`). Scope: `users:admin` or
    `admin` wildcard.
  - `GET /v1/jobs/{job_key}/stats?days=N` — total / completed /
    failed / dead, success_rate, p50/p95/p99 duration, last failure
    timestamp. Default window 7 days, clamped to [1, 90].
  - `GET /v1/executions/throughput?window=24h|7d|30d` — stacked
    `{ok, err}` buckets aligned to UTC hour/day starts.
  - `GET /v1/insights/failures?days=N` — 2D heatmap rows (day × hour
    of UTC), plus top-3 hourly hotspots. Default 28 days,
    clamped to [7, 90].
  New `audit_log` table (migration 016) — append-only, indexed on
  `created_at`, `(target_type, target_id)`, `(actor_type, actor_id)`.
  Mutation handlers in subsequent PRs will call
  `audit::record(...)` to populate it.

- **Optional SMTP transport for invitations + password-reset (PR-A6).**
  New cargo feature `smtp` gates a lettre-backed `SmtpSender`. When
  the feature is built in AND `CRONIQ_SMTP_URL` + `CRONIQ_SMTP_FROM`
  are set, outbound mail is sent for real; otherwise the `NoopSender`
  default keeps working and the token URL still comes back in the
  API response (the explicit-fallback mode the operator picked over
  SMTP-mandatory in the spike Q&A). URL format follows lettre's
  conventions: `smtp://user:pass@host:587/?tls=required`.

- **OIDC/SSO login (PR-A5).** Manual Authorization-Code flow against
  any OpenID-Connect provider with Discovery — tested mentally
  against Authentik, Keycloak, Auth0. New env-only config (a DSL
  `oidc {}` block follows in PR-A5b):
  - `CRONIQ_OIDC_ISSUER` — base URL, `.well-known/openid-configuration` is appended
  - `CRONIQ_OIDC_CLIENT_ID`, `CRONIQ_OIDC_CLIENT_SECRET`, `CRONIQ_OIDC_REDIRECT_URL`
  - `CRONIQ_OIDC_DEFAULT_ROLE` (default `viewer`), `CRONIQ_OIDC_PROVIDER_NAME` (default `oidc`)
  - When any required var is missing, OIDC stays disabled and the
    routes return 404.

  New endpoints:
  - `GET /v1/auth/oidc/config` — read-only metadata (`enabled`,
    `provider_name`, `login_url`) for the login UI's "Sign in with SSO" button.
  - `GET /v1/auth/oidc/login` — 302-redirect to the IdP's authorize
    URL. Random `state` + `nonce` persisted in `oidc_pending_logins`
    (TTL 10 min, single-use take-and-delete).
  - `GET /v1/auth/oidc/callback?code=&state=` — atomic state lookup,
    token exchange, JWKS-based ID-token verify (RS256/384/512), nonce
    check, userinfo fetch. Returns the standard `TokenResponse`.

  JIT user provisioning: first sign-in creates a `users` row with
  `role=viewer` (or whatever `CRONIQ_OIDC_DEFAULT_ROLE` sets). The
  link lives in `oidc_identities (provider, subject) → user_id`.
  Username collision with an existing local password user is refused
  with 409 to prevent silent account hijack.

  Schema: `015_oidc.sql` (oidc_identities + oidc_pending_logins).
  Dependencies: `reqwest` (rustls), `jsonwebtoken`, `base64`, `rand`.
  `auth_method=oidc` is set on every OIDC-issued token so the audit
  log distinguishes SSO sessions from password ones.

- **Personal Access Tokens (PR-A4).** User-bound API credentials,
  distinct from `api_keys` (which belong to service identities). PATs
  carry a stable `user_id`, a human label ("laptop", "ci-personal"),
  and a scope subset of the owning user's role's default-scope set —
  a Viewer can't mint a PAT with `jobs:write`, the API refuses with 403.
  - `POST /v1/users/me/tokens` — issue. Raw `croniq_pat_…` token is
    returned ONCE; only the SHA-256 hash is persisted.
  - `GET /v1/users/me/tokens` — list the caller's tokens.
  - `DELETE /v1/users/me/tokens/{id}` — revoke.
  - Auth middleware accepts `Authorization: Bearer croniq_pat_…` and
    the explicit `Authorization: PAT …` header. `last_used_at` is
    stamped best-effort on every successful request.
  New migration: `014_personal_access_tokens.sql`. `CallerType::User`
  with `auth_method: pat` distinguishes PAT-authenticated requests
  from password-authenticated sessions in the audit log.

- **TOTP/2FA with single-use recovery codes (PR-A3).** Users can
  enable a second factor in self-service. The login flow becomes
  two-step when 2FA is on:
  - `POST /v1/auth/login` returns
    `{ requires_totp: true, mfa_token, mfa_token_expires_in }` instead
    of access tokens. The MFA token is a short-lived JWT
    (`purpose: "mfa"`, 5 min TTL) that `validate_token` rejects for
    every other endpoint.
  - `POST /v1/auth/login/totp` exchanges the MFA token + a 6-digit
    code (or single-use recovery code) for normal access + refresh
    tokens.
  - `POST /v1/users/me/totp/setup` returns the base32 seed, an
    `otpauth://` URL for QR-code rendering, and 10 fresh recovery
    codes (8 lowercase alphanumerics each). Idempotent until
    confirmed.
  - `POST /v1/users/me/totp/confirm` (body `{ code }`) flips
    `enabled=true`.
  - `POST /v1/users/me/totp/disable` (body `{ password }`) requires
    fresh password proof and wipes the secret + all recovery codes.
  - `POST /v1/users/me/totp/recovery-codes/regenerate` (body
    `{ password }`) mints a new set, invalidating the previous batch.
  TOTP secrets are wrapped at rest with AES-256-GCM using a key
  derived from `CRONIQ_JWT_SECRET` via HKDF-SHA256
  ([`croniq-auth::crypto`](crates/croniq-auth/src/crypto.rs)).
  Recovery codes are SHA-256 hashed and case-/whitespace-normalised
  for paste-from-PDF UX. New migration:
  `013_totp_and_recovery.sql`.

- **User-CRUD + Invitations + Password-Reset (PR-A2).** New endpoints
  build on the role model from PR-A1 so a workspace admin can grow the
  team beyond the seeded admin user:
  - `GET/POST /v1/users`, `GET/PATCH/DELETE /v1/users/{id}` — admin-only.
    PATCH and DELETE refuse with 409 Conflict when the operation would
    leave zero active admins (role-demotion, deactivation, deletion).
  - `GET /v1/users/me`, `PATCH /v1/users/me`, `POST /v1/users/me/change-password`
    — self-only (display name + email + own password).
  - `POST/GET /v1/invitations`, `DELETE /v1/invitations/{id}` (admin),
    `POST /v1/invitations/accept` (public, body `{token, username, password}`).
    Invitations carry a single-use SHA-256-hashed token; the raw token
    is returned **once** in the create response together with the
    pre-built `accept_url`. Expiry: 7 days.
  - `POST /v1/auth/password-reset/request` (always returns 202 to avoid
    user-enumeration), `POST /v1/auth/password-reset/confirm`. Reset
    tokens live 1 hour, single-use.
  - New `users:admin` scope. `admin` wildcard still implies it.
  - New migration `012_invitations_and_resets.sql`.
  ([`crates/croniq-server/src/api/users.rs`](crates/croniq-server/src/api/users.rs),
  [`invitations.rs`](crates/croniq-server/src/api/invitations.rs),
  [`password_reset.rs`](crates/croniq-server/src/api/password_reset.rs))

- **`EmailSender` trait with `NoopSender` default.** Outbound mail is
  abstracted behind a trait so PR-A6 can drop in an `lettre`-backed
  `SmtpSender` behind the `smtp` cargo feature. Until then, every
  invite + reset endpoint returns the token URL in the API response
  (and logs an audit line) so admins can deliver it out-of-band.
  ([`crates/croniq-server/src/email.rs`](crates/croniq-server/src/email.rs))

- **Multi-user identity model with role-based scopes** — new `users` table
  (migration 011) splits identity from credentials. Three roles map to
  pre-defined scope sets via `croniq_auth::default_scopes_for_role`:
  `admin` (wildcard, unchanged), `operator` (read everything + write
  jobs/schedules/calendars + trigger), `viewer` (read-only across the
  board). The login handler embeds the user's role-scopes in the JWT
  instead of the previous hardcoded `["admin"]`; existing single-admin
  deploys are backfilled into `users` with `role=admin` so behaviour is
  preserved. User-CRUD endpoints (`/v1/users`, `/v1/users/me`, invite
  flow, TOTP, OIDC) land in follow-up PRs A2-A5.
  ([`crates/croniq-store/src/migrations/011_users.sql`](crates/croniq-store/src/migrations/011_users.sql),
  [`crates/croniq-auth/src/context.rs`](crates/croniq-auth/src/context.rs))

- **`CallerContext` now carries `user_id`, `role`, and `auth_method`.**
  Audit-log consumers, alert routing, and the upcoming `/v1/users/me`
  endpoint depend on knowing which user (not just which client) made a
  request, and which auth method was used (password / API key / PAT /
  OIDC — the latter two reserved for follow-up PRs but enumerated now
  to avoid a breaking JSON change later).
  ([`crates/croniq-auth/src/context.rs`](crates/croniq-auth/src/context.rs))

### Changed

- **BREAKING — JWT issuer hard-cut.** The issuer claim moves from
  `"croniq"` to `"croniq-v1"` to invalidate tokens minted before the
  Multi-User schema landed (those tokens lack the new `user_id` / `role` /
  `auth_method` claims). All UI sessions are forced to re-login on the
  first request after upgrade; API-key authentication is unaffected
  because it bypasses JWT validation. The bump is centralised in
  `croniq_auth::JWT_ISSUER` — future migrations follow the same `-vN`
  pattern.
  ([`crates/croniq-auth/src/jwt.rs`](crates/croniq-auth/src/jwt.rs))

### Fixed

- **`apiFetch` no longer treats `204 No Content` as a JSON parse
  failure
  ([#145](https://github.com/nuetzliches/croniq/pull/145)).** DELETE
  responses with empty bodies used to surface "Unexpected end of JSON
  input" toasts to operators; the helper now short-circuits on 204
  and on empty `Content-Length: 0`. Bundled with the same PR: topbar
  visual polish (badge alignment + collapsed-sidebar hit target,
  sidebar collapse styles forced under 768 px viewport).

### Docs

- **Hookaido positioning clarified — inbound-only
  ([#142](https://github.com/nuetzliches/croniq/pull/142)).** Hookaido
  is a plugin / module that accepts incoming webhook payloads and
  turns them into job triggers; it is **not** an outbound
  alert-delivery transport. README + ROADMAP were updated to remove
  the misleading "bridge" wording. Outbound alerts are delivered by
  Croniq's own shell / webhook / email channels (see Failure alerts
  entry above).

## [0.14.0] - 2026-05-21

### Changed

- **Official Docker image and release binaries now ship with the `otlp`
  feature compiled in.** `ghcr.io/nuetzliches/croniq:latest` (and the
  `croniq-*-{tar.gz,zip}` archives consumed by Homebrew) honour
  `OTEL_EXPORTER_OTLP_ENDPOINT` out of the box; setting the endpoint env
  var activates the OTLP span + log exporters at runtime, no rebuild
  required. The runtime gate in
  [`telemetry.rs::decide`](crates/croniq-server/src/telemetry.rs) keeps
  the layer dormant when the endpoint is unset, so behaviour is identical
  to the previous off-build for operators who do not opt in. Cargo
  default stays off so a checkout `cargo build` does not pull the
  opentelemetry stack — pass `--features croniq-server/otlp` explicitly
  when building from source
  ([#124](https://github.com/nuetzliches/croniq/issues/124)).

### CI

- The `Build (otlp feature)` step is now the primary workspace build;
  added a `cargo build -p croniq-server` (no-default-features) smoke +
  matching `cargo test -p croniq-server` to keep the
  `#[cfg(not(feature = "otlp"))]` branches in `telemetry.rs` exercised
  on every push.

## [0.13.0] - 2026-05-21

### Added

- **Optional OTLP exporter for traces + logs** — new
  [`croniq-server`](crates/croniq-server) Cargo feature `otlp` (off by
  default) installs OTLP span and log exporters in parallel with the
  existing stderr `tracing-subscriber` fmt layer. Configuration is
  driven entirely by the standard W3C / OpenTelemetry environment
  variables — no Croniqfile changes:
  - `OTEL_EXPORTER_OTLP_ENDPOINT` — if set, OTLP layers are installed;
    if unset, behaviour is identical to pre-0.13.
  - `OTEL_EXPORTER_OTLP_PROTOCOL` — `grpc` (default, port 4317) or
    `http/protobuf` / `http/json` (port 4318). Both transports are
    compiled into the `otlp` feature, so the choice is purely runtime.
  - `OTEL_SERVICE_NAME` (defaults to `croniq`),
    `OTEL_RESOURCE_ATTRIBUTES`, and `OTEL_LOG_LEVEL` (separate
    EnvFilter for the OTLP log bridge so `RUST_LOG=trace` does not
    flood the collector).

  `Scheduler::tick` and `CompletionProcessor::process` carry
  `#[tracing::instrument]` annotations so operators can trace a job
  from schedule → fire → dispatch → ack → outcome in their collector.
  A `TelemetryGuard` flushes the OTLP batch exporters after
  `axum::serve` returns so in-flight spans are not dropped on
  SIGINT/SIGTERM. Targeted at users running Croniq alongside .NET
  Aspire, Grafana Tempo/Loki, or any OTLP-speaking collector — see the
  [README "Observability" section](README.md#observability) for the
  full env-var matrix and Aspire example
  ([#121](https://github.com/nuetzliches/croniq/issues/121),
  [#122](https://github.com/nuetzliches/croniq/pull/122)).

### Notes

- The default release binaries / Docker images currently ship **without**
  the `otlp` feature. Operators who want OTLP today need to build with
  `cargo install --path crates/croniq-server --features otlp` or build a
  custom image with the feature flag. Whether to flip the default for
  the official images is tracked as a follow-up decision; the change is
  zero-cost at runtime when the endpoint env var is unset.
- Out of scope for this release (tracked separately on the ROADMAP):
  OTLP metrics push, W3C `traceparent` propagation between server and
  runners, and alignment with the stabilising `messaging.*` / `cron.*`
  OpenTelemetry semantic conventions.

## [0.12.0] - 2026-05-12

### Added

- **Croniqfile `mcp { allowed_hosts ... }` directive** — explicit
  `Host`-header allowlist for the `/mcp` Streamable-HTTP transport,
  resolving the workaround documented in v0.10.1. Empty / absent list
  keeps rmcp's loopback-only default (`localhost`, `127.0.0.1`, `::1`);
  entries listed in the directive are **appended** to the default — they
  do not replace it, so an operator who lists their public hostname does
  not lose local debugging access. Wildcards are not supported; enumerate
  every public hostname explicitly. IPv6 literals with port require
  quoting (`"[::1]:8443"`). Example:

  ```
  mcp {
    enabled true
    allowed_hosts cron.internal admin.example.com
  }
  ```

  Closes [#114](https://github.com/nuetzliches/croniq/issues/114),
  [#116](https://github.com/nuetzliches/croniq/pull/116).
- **Runner SDK: streaming log writer** —
  [`ExecutionContext::log_writer`](crates/croniq-runner-sdk/src/handler.rs)
  returns a cloneable `LogWriter` handle backed by a bounded
  `tokio::sync::mpsc` channel and a background flusher task. Calls to
  `writer.send(level, msg).await` only suspend on channel capacity,
  never on HTTP — eliminating the previous trade-off where SDK-based
  runners wrapping long-running subprocesses had to choose between
  batch-at-end (no live progress) and per-line `ctx.log().await` (which
  backpressures the stdout reader into a self-induced deadlock when the
  server is slow). The flusher batches by size (32 events), time
  (200 ms), or explicit `writer.flush().await`, with a hard cap of 100
  events per POST. The runner deterministically drains the writer (up
  to 5 s) before sending `ack`, so logs are server-side by the time the
  execution is marked complete. Existing `ctx.log()` and
  `ctx.push_log_events()` are unchanged
  ([#115](https://github.com/nuetzliches/croniq/issues/115),
  [#117](https://github.com/nuetzliches/croniq/pull/117)).
- **`LogWriter::null()`** — public no-op constructor that silently
  drains every event. Useful for unit tests where a function takes a
  `&LogWriter` but the test asserts on side-effects elsewhere (e.g.
  `croniq-shell-runner`'s exec tests asserting on the tail buffer)
  ([#119](https://github.com/nuetzliches/croniq/pull/119)).

### Changed

- **`croniq-shell-runner` now streams stdout/stderr live** — every
  line emitted by a `runner shell { ... }` / `runner exec { ... }`
  subprocess goes through the SDK's `LogWriter` as it appears, so the
  Execution Detail Logs panel renders chatty / long-running jobs (CVE
  scans, restic backups, multi-minute test runs) incrementally instead
  of all-at-once at process exit. `exec::run` no longer uses
  `wait_with_output`; it spawns the child and reads stdout/stderr via
  `tokio::io::BufReader::lines`, mirroring each line into (a) the
  runner's own container logs (`tracing::info!` with stream-specific
  targets so a sidecar shipper picks them up), (b) a rolling 50-line
  tail buffer per stream for failure-snippet assembly, and (c) the
  streaming `LogWriter`. `Outcome` now exposes `stdout_tail` /
  `stderr_tail` (`VecDeque<String>`, last 50 lines each) instead of
  the full strings; failure messages remain identical in shape
  (`exit {code}: {last 400 chars of stderr}`). Backpressure for slow
  servers propagates safely from the writer's bounded channel back
  through the OS pipe to the child's `write()` syscall — the
  pattern-B deadlock described in #115 cannot occur
  ([#118](https://github.com/nuetzliches/croniq/issues/118),
  [#119](https://github.com/nuetzliches/croniq/pull/119)).

## [0.11.0] - 2026-05-10

### Added

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
  ([#108](https://github.com/nuetzliches/croniq/issues/108),
  [#111](https://github.com/nuetzliches/croniq/pull/111)).
- **Auto-injected `runner_id` and `runner_tags` on every log event** —
  `ExecutionContext::push_log_events` already auto-injected `job_key`;
  now also injects which runner instance produced the event and that
  runner's self-declared tags as a JSON-array string. Loki / CloudWatch
  / OpenSearch users get free filterable structured logs without
  threading these values through every call site. Callers can still
  override either field by setting it explicitly on `WorkEvent.fields`
  ([#109](https://github.com/nuetzliches/croniq/pull/109)).

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
  config ([#110](https://github.com/nuetzliches/croniq/pull/110)).

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
  still overrides everything
  ([#103](https://github.com/nuetzliches/croniq/issues/103),
  [#105](https://github.com/nuetzliches/croniq/pull/105)).
- **Dead-letter writes now happen atomically with the dead-state
  transition, and orphans are backfilled** — the completion processor
  previously made two separate non-transactional store calls (`UPDATE
  executions SET state='dead'` followed by `INSERT INTO dead_letters`)
  and swallowed errors with `let _ = ...`. Failures of the second
  call left `state='dead'` rows with no corresponding dead-letter, so
  the Dead Letters UI page stayed empty even when actionable failures
  existed. New `DeadLetterStore::complete_as_dead` runs both writes
  in one transaction; SQLite migration `009_backfill_dead_letters`
  populates the table for existing orphan rows on first start
  (`expires_at = NULL` so the purge sweeper leaves them alone).
  Errors are now logged at `tracing::error!` level
  ([#104](https://github.com/nuetzliches/croniq/issues/104),
  [#106](https://github.com/nuetzliches/croniq/pull/106)).
- **Dead-letter retention TTL is now actually enforced** — the
  watchdog tick (every 30s) calls `store.purge_expired(now)` so rows
  whose `expires_at` has passed are reaped. The function existed
  since v0.9 but was never called from the server's main loop,
  meaning `dead_letter { retention 14d }` was a documented setting
  with no effect
  ([#104](https://github.com/nuetzliches/croniq/issues/104),
  [#106](https://github.com/nuetzliches/croniq/pull/106)).
- **Differentiated empty-logs message in the Execution Detail panel** —
  the shared "No logs for this execution" line implied a missing-data
  bug for *every* zero-log execution, including completed runs whose
  runners simply produced no stdout. Operators interpreted silent
  healthy runs as broken instrumentation. The message now splits by
  state: `completed` → "Silent run completed (no stdout)", `failed`/
  `dead` → "No logs captured", anything else → "No logs yet"
  ([#107](https://github.com/nuetzliches/croniq/pull/107)).

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
