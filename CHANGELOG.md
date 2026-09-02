# Changelog

All notable changes to Croniq are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **.NET: trigger `metadata` values widen from `string` to `object?`
  ([#554](https://github.com/nuetzliches/croniq/issues/554)).** The server
  forwards trigger metadata to the handler verbatim and explicitly does not
  flatten or stringify it, and every other SDK types the value openly — Go
  `map[string]any`, Python `dict[str, Any]`, TypeScript
  `Record<string, unknown>`, Java `Map<String, Object>`. .NET was the outlier
  and could not send the nested or non-string values the wire contract allows.

  **Source-breaking** for callers passing a `Dictionary<string, string>`: change
  it to `Dictionary<string, object?>`. Values already being strings continue to
  serialise identically, so no wire behaviour changes for existing payloads.

### Fixed

- **The .NET SDK now runs the shared trigger conformance corpus
  ([#554](https://github.com/nuetzliches/croniq/issues/554)).** It was the only
  SDK that ran none of it: the project copied and enumerated
  `conformance/cases/` — the runner corpus — and had no trigger case type, no
  trigger runner, and no `body_absent` support anywhere.

  `body_absent` is the assertion that pins "a producer must not fabricate
  defaults on the wire" — the contract
  [#551](https://github.com/nuetzliches/croniq/issues/551) depended on and
  [#553](https://github.com/nuetzliches/croniq/issues/553) extended to
  explicitly empty values. On .NET it rested on a single unit test instead. Not
  a hypothetical gap: the conformance README records that every binding once
  *parsed* `body_absent` without asserting it (fixed in #460, with Go
  live-affected), and a binding that does not run the corpus at all is the same
  hole one step further back.

  Wiring it in immediately surfaced a latent bug in the **shared** case loader:
  `CoerceScalar` coerced numeric strings but left `"true"` a string, so a
  scripted `deduplicated: true` was re-serialised as the JSON *string* `"true"`
  and the SDK refused it with *"Cannot get the value of a token type 'String'
  as a boolean"*. The runner corpus carries no booleans in its scripted response
  bodies, so nothing had ever exercised that path — two trigger cases failed on
  it before the fix.

  The new loader is strict (no `IgnoreUnmatchedProperties`) like its runner
  counterpart, so a schema-legal key this binding has not implemented fails at
  load rather than going silently unasserted, and the
  unset-vs-explicitly-empty distinction is pinned in both directions so #553's
  case cannot decay into asserting omission of something never supplied. The
  theory also guards its own discovery — an empty case list would make it
  vacuously green, which is precisely the shape of the gap being closed.

- **Every SDK now omits an explicitly *empty* trigger optional instead of
  sending it, and a blank `timeout` no longer reaches the runner unparseable
  ([#553](https://github.com/nuetzliches/croniq/issues/553)).** The five SDKs
  agreed on how to send an *unset* optional — all omit it, pinned by
  `cases-trigger/01-trigger-minimal.yaml`. They disagreed on an *empty* one: Go
  dropped `require: []` and `timeout: ""` via `omitempty`, while Rust, Python,
  TypeScript, Java and .NET sent them verbatim.

  For the capability lists this was redundant rather than wrong — the server has
  read an empty `require` as "inherit the job's `runner { require … }`" since
  [#549](https://github.com/nuetzliches/croniq/issues/549), so `"require": []`
  was only a second wire spelling of a message that already had one. `timeout`
  was the case with teeth: since
  [#551](https://github.com/nuetzliches/croniq/issues/551) an absent `timeout`
  means "inherit the job's", so a present one is an explicit override — and
  `""` is not a parseable duration. Five of six clients could hand the runner a
  broken timeout where Go's would have inherited `timeout 2h`.

  Resolved as **empty means absent**, normalized off the wire, which makes Go's
  behaviour the reference rather than the outlier. The alternative — optional
  collection types so Go *could* send `[]` — buys a distinction the server has
  no meaning for, breaks Go's public API, and leaves the `timeout: ""` hazard
  standing. No SDK's public signatures change; the normalization is internal
  (Rust in the builder setters, Java in the record's compact constructor so
  direct construction is covered too, .NET in a `TriggerRequest.Normalized`
  factory, Python and TypeScript at the body-build step).

  The server hardens to match: `POST /v1/trigger` and the two MCP fire tools
  treat a blank `timeout` — whitespace included — as absent, so a hand-rolled
  request or a non-conforming client inherits the job's timeout rather than
  producing a work item nothing can parse. This is the rule `idempotency_key`
  already applied ("an empty string is treated as absent"), now consistent
  across its neighbours.

  Pinned by a new `cases-trigger/12-trigger-empty-optionals.yaml`, which failed
  on five of six bindings before this change. It needed no schema change — the
  existing `body_absent` assertion already expressed it.

- **A manually fired execution now inherits its job's configured `timeout`
  instead of always getting 5 minutes
  ([#551](https://github.com/nuetzliches/croniq/issues/551)).** `timeout` on
  `TriggerRequest` carried a serde default of `"5m"`, so by the time the
  handler saw it an omitted field and a caller who deliberately sent `5m` were
  the same value — there was no "unset" to fall back from. A job declaring
  `timeout 2h` ran for two hours when the scheduler fired it and was killed
  after five minutes when someone triggered it by hand: exactly the on-demand
  backfill or replay of a long job where it hurts, and it reads as a runner
  problem rather than a routing default.

  This is the half deliberately left out of
  [#549](https://github.com/nuetzliches/croniq/issues/549), because unlike
  `require` it could not lean on "empty means unset" — an empty capability list
  has no caller intent, `"5m"` does. `TriggerRequest::timeout` and the two MCP
  fire-tool params are now `Option<String>`, resolved as
  request → job config → `5m`, the precedence the dead-letter replay path
  already used.

  **Wire-compatible**: an absent field deserialises to `None`, a present one to
  `Some`, and a caller who genuinely wants 5 m for a 2 h job can still say so.
  A **Rust API break** for anyone constructing `TriggerRequest` literally. All
  five language SDKs already omit `timeout` when the caller gives none — the
  conformance suite has pinned that from the start
  (`cases-trigger/01-trigger-minimal.yaml`, and `05-trigger-timeout.yaml` spells
  out that a client "must OMIT timeout … not fabricate the server default") — so
  the wire contract already assumed the server would honour the omission. It
  just could not.

  `croniq trigger --timeout` loses its `"5m"` default for the same reason: the
  CLI was the one first-party client that fabricated one, which would have
  defeated the fix for every command-line fire. Omitting the flag now inherits
  the job's timeout.

- **`POST /v1/trigger` now inherits a job's `runner { require … }` — a manually
  triggered job could be claimed by any runner
  ([#549](https://github.com/nuetzliches/croniq/issues/549)).** `handle_trigger`
  built the work item with the caller's `require` field verbatim and no
  fallback. That field is `#[serde(default)]`, so a trigger that simply names a
  job key produced an *empty* `require` — and an empty `require` matches every
  runner in `Queue::dequeue_for_where`. A job pinned to `require api-x` ran on a
  runner that had no `api-x`: wrong host, missing credentials, missing binary,
  and nothing logged, because from the queue's perspective there was no
  requirement to violate.

  Capabilities could not ride in on the metadata inheritance that already makes
  `__runner_exec` ([#89](https://github.com/nuetzliches/croniq/issues/89)) and
  `__max_concurrent` work on this path: the compiler never stamps them into
  `job.metadata` — the scheduler adds `__require` / `__prefer` only as it
  persists the execution row — so there was nothing there to inherit.

  The trigger path was the only work-item producer without the fallback. The
  scheduler fire, the retry, the watchdog requeue and the dead-letter replay all
  read `job.runner.require`; the watchdog is the clearest evidence that
  inheritance was the intent, since an abandoned triggered execution came back
  with the *correct* `require` after a requeue — the same execution routing
  differently before and after the watchdog touched it.

  An omitted `require` / `prefer` now falls back to the job's runner config; an
  explicit value in the request still overrides, as before. The effective
  capabilities are also stamped into the persisted row's metadata the way the
  scheduler does, so the in-memory queue and the store-side claim filter agree
  and a later dead-letter replay can read them off the row instead of depending
  on the job still existing. Caller metadata in the reserved `__` namespace is
  dropped as it already was, so the stamp is purely additive.

  The MCP `job_trigger` and `enqueue_job` tools had the same gap, and one more
  besides: they enqueued without the job's compiled metadata, so an MCP-fired
  shell job reached the runner with no `__runner_exec` — no command to run —
  and persisted an execution row with no metadata at all. Both now inherit the
  job config the way `dlq_retry` in the same file always has: metadata as the
  base with caller metadata overlaid, capabilities as the fallback, and the
  effective set stamped onto the persisted row.

  `timeout` keeps its existing behaviour on both paths: it defaults to `"5m"`,
  so a triggered execution still ignores the job's configured timeout and the
  server cannot distinguish an omitted field from a literal `5m`. Closing that
  needs an `Option<String>` on the public `TriggerRequest` and is tracked
  separately.

### Security

- **OIDC discovery now requires TLS on every endpoint it is handed.** The
  discovery document at `<issuer>/.well-known/openid-configuration` is fetched
  over the network and then trusted for four URLs this server calls:
  `authorization_endpoint`, `token_endpoint`, `jwks_uri` and
  `userinfo_endpoint`. The issuer-match check that already ran proves the
  document belongs to the configured issuer; it said nothing about the scheme
  of the endpoints inside it.

  That gap mattered most at `jwks_uri`, which decides whose signature counts
  as a valid ID token: a plaintext fetch lets anyone on the path serve their
  own key set and mint logins for any account. `token_endpoint` carries the
  `client_secret` as Basic auth. Both, plus the configured issuer itself, must
  now be `https://`; discovery fails with a message naming the offending
  endpoint.

  Endpoints are deliberately **not** required to share the issuer's host —
  real providers split them (Google's issuer is `accounts.google.com` while
  its JWKS lives on `www.googleapis.com`), so host pinning would reject valid
  deployments without closing anything https does not already close.
  Loopback issuers stay exempt, so a local `http://localhost:8080` Keycloak or
  Authentik still works for development.

  The token exchange and the userinfo call additionally refuse to follow HTTP
  redirects, so neither the `client_secret` nor the access token can be
  bounced to another host. Discovery and JWKS keep the default redirect
  policy — they carry no credential.

  Raised by CodeQL as `rust/request-forgery` (alerts 40, 41).

## [0.36.0] - 2026-08-31

### Added

- **The scheduler heartbeat now reports what became of each ephemeral fire,
  not just how many there were
  ([#541](https://github.com/nuetzliches/croniq/issues/541)).** The dispatch
  bug in [#539](https://github.com/nuetzliches/croniq/issues/539) went
  unnoticed for six minor releases because nothing could see it: an ephemeral
  job keeps no execution history by design, so one that fires and never
  reaches a runner looks exactly like one running perfectly — on the
  dashboard, in `GET /v1/executions`, and in the job list alike. The v0.29.0
  heartbeat counted fires, which is the scheduler's own side of the hop, and
  fires were never the half that was broken.

  Both ends are now counted — the scheduler when it enqueues, the poll path
  when it hands work out — and folded into the same `INFO` heartbeat
  (~5 min), so the format of its `ephemeral=` field changes from
  `[<key>:N, …]` to one `; `-separated entry per job:

  ```text
  scheduler heartbeat — alive ephemeral=[beat:tick fired=300 dispatched=299 superseded=1]
  ```

  `fired` and `dispatched` are always shown so the two can be compared at a
  glance; `dropped` and `superseded` appear only when non-zero. **`fired=N
  dispatched=0` is the signature of #539.** `superseded` counts fires
  replaced by a newer one before any runner claimed them (the "keep only the
  latest" rule from
  [#263](https://github.com/nuetzliches/croniq/issues/263)) — expected
  whenever runners poll slower than a job fires, and the honest explanation
  for a `fired` that exceeds `dispatched` on a healthy server, so the numbers
  add up instead of crying wolf. A `dropped` fire additionally logs a `WARN`
  naming the job key, since after #539 there is no benign reason for one.

## [0.35.2] - 2026-08-31

### Fixed

- **`ephemeral` jobs are dispatched again — since v0.29.0 the poll path
  silently dropped every one of them
  ([#539](https://github.com/nuetzliches/croniq/issues/539)).** The scheduler
  fired them and queued the work item as designed, but dispatch then validated
  each item against a store row and dropped it when the store answered
  `NotFound` — which is the *expected* answer for an ephemeral execution:
  `ExecutionMode::Ephemeral` skips the insert by definition and tracks the id
  in `ephemeral_inflight` instead. So every ephemeral fire ended in
  `work item dropped — execution is no longer queued in the store`, one WARN
  per fire, and no runner ever saw the work.

  The mode is documented as skipping *persistence*, not dispatch, and the
  failure was invisible from the outside: ephemeral jobs have no execution
  history by design, so a job that never ran once looks exactly like one
  running perfectly. An `ephemeral every 1 minute` job on a green server could
  do nothing for months.

  Work items now carry `is_ephemeral`, set from the job's execution mode where
  the item is built, and dispatch skips the store claim for those instead of
  interpreting its answer — there is no row to claim, and never will be. This
  also stops the `ephemeral_inflight` entries leaking: a dropped item never
  reported a completion, so its tracking entry lingered until the max-age
  sweep. The refused-claim guard from
  [#374](https://github.com/nuetzliches/croniq/issues/374) is unchanged for
  persisted executions — a `Conflict`, or a missing row for a queued
  execution, still drops the item. Every other producer of a work item
  (retry, requeue, replay, `POST /v1/trigger`, MCP enqueue) writes its
  execution row first and is unaffected.

## [0.35.1] - 2026-08-27

### Fixed

- **Shortening a job's interval now takes effect on the next reload instead of
  after the old, longer fire elapses
  ([#535](https://github.com/nuetzliches/croniq/issues/535)).** A running job
  carries a pending fire time, and a reload (or a restart, which reads it back
  from `job_states`) kept it so the job neither skips nor double-fires while
  the config changes underneath it. It was kept unconditionally, though — so an
  interval edited *downwards* stayed on the old cadence for one more round:
  `every 1 hour` → `every 1 minute` bought up to an hour of silence, `every 1
  day` → `every 1 hour` up to a day.

  What made it expensive is that nothing said so. The Croniqfile, the
  `configuration loaded` line, `GET /v1/schedules` and `is_active` on
  `GET /v1/jobs` all report the new schedule as active for the whole window,
  restarting the container does not clear it (the instant is persisted, not
  in-memory), and the failure is indistinguishable from a job that is simply
  broken.

  A carried-over instant is now adopted only while it can still belong to the
  schedule just loaded — that is, while it is no later than that schedule's own
  next fire. `compute_next_fire` is monotone in its argument, so a later
  instant provably came from a schedule (or a calendar/window gate) that no
  longer applies, and is recomputed from now with an `INFO` naming the job. The
  check only ever moves a pending fire *earlier*, so nothing due can be lost by
  it: an already-overdue fire is a missed fire and still goes to
  `misfire_policy`, and lengthening an interval still runs the sooner fire the
  old schedule promised before picking up the new cadence. Both load paths are
  covered — hot-reload (`--watch`, `SIGHUP`, `POST /v1/admin/reload-config`)
  and the restart-time restore from `job_states`, where the recomputed instant
  is persisted immediately so `GET /v1/jobs` and the missed-fire watchdog stop
  reporting the stale one.

## [0.35.0] - 2026-08-23

### Added

- **Rotating `CRONIQ_JWT_SECRET` no longer costs every enrolled user their
  second factor ([#531](https://github.com/nuetzliches/croniq/issues/531)).**
  The at-rest key for stored TOTP seeds is HKDF-derived from the signing
  secret, so rotating the secret rotated the wrap key with it. The coupling is
  deliberate — anyone who can read the signing secret can already mint admin
  tokens — but there was no path that re-wrapped the seeds, so the only
  documented procedure was: relax `auth { totp { required false } }`, rotate,
  have every user re-enrol, re-enable. That lowers the security posture during
  exactly the window where it should be highest, and it scales with the number
  of users rather than the number of operators.

  Name the outgoing value in **`CRONIQ_JWT_SECRET_PREVIOUS`** (or its `_FILE`
  sibling) and the server re-wraps every stored seed under the new key at boot,
  before it accepts traffic:

  ```
  INFO re-wrapped stored TOTP secrets under the current JWT secret
       rewrapped=7 already_current=0 write_failed=0 undecryptable=0
  INFO CRONIQ_JWT_SECRET_PREVIOUS can now be removed.
  ```

  A rotation becomes: set both → restart → drop the old value. Enforced 2FA
  stays on throughout and nobody re-enrols. The variable is used to *unwrap*
  only — it never signs a token, never validates one, and never wraps a new
  secret, so anything enrolled after the rotation is under the current key by
  construction.

  The sweep is idempotent and never fails the boot: a row the store refuses to
  write is logged and skipped, because a failed convenience migration should
  not become an outage. Those rows still authenticate — the login path falls
  back to the previous key, warns with the user id, and re-wraps the row on the
  way through. A `CRONIQ_JWT_SECRET_PREVIOUS` that is *not* the value the rows
  were stored with still fails closed.

  This is also the cheap way out of the #408 upgrade path: a deployment that
  fell through to a freshly generated `$DATA_DIR/jwt.secret` can name the old
  `pull_api { auth … }` value and recover every enrolment.

- **`doctor` reports stored TOTP secrets as a positive, and separates
  "pending a re-wrap" from "lost".** `totp.secrets_undecryptable` used to be
  the only thing said about stored seeds, and only when something was wrong.
  Two findings join it: `totp.secrets_under_previous_key` (Warning — these
  authenticate through the fallback, but the rotation is unfinished and
  `CRONIQ_JWT_SECRET_PREVIOUS` cannot be dropped yet) and
  `totp.secrets_decryptable` (Info — *N stored TOTP secret(s), all decryptable
  under the current key*), so a completed rotation is visible rather than
  merely un-complained-about. The remedy on `totp.secrets_undecryptable` now
  leads with the re-wrap instead of a re-enrolment campaign.

### Fixed

- **The Docker entrypoint now says when `CRONIQ_ADMIN_PASSWORD` is being
  ignored ([#530](https://github.com/nuetzliches/croniq/issues/530)).**
  `CRONIQ_ADMIN_PASSWORD` and `CRONIQ_INIT_API_KEY` are seed credentials:
  `croniq init` reads them when the entrypoint creates the database and nothing
  reads them again. That part is right — re-applying a bootstrap credential on
  every start would be worse — but it happened in silence, and the value goes
  on living in a compose `.env`, a CI secret, or a secret-manager entry. From
  there, "matches the password in force", "was rotated in the UI months ago"
  and "was never right, and the database was seeded with the generated
  fallback" look identical, and all three appear to work, because nothing reads
  the value any more.

  Every start where the database already exists and either variable is set
  (directly or through its `_FILE` sibling) now prints a `NOTE` on stderr
  naming the variable and pointing at the rotation path that does work —
  Settings in the UI or `POST /v1/users/me/change-password` for the password,
  Settings → API Keys or `croniq api-keys` for the key. `README.md` and
  `docs/operations.md` mark both variables seed-only where they were previously
  listed alongside the live SMTP settings.

- **An SDK release no longer takes over the repository's "Latest", which broke
  `install.sh`.** Publishing the v0.34.0 cut put five SDK releases out after
  the server one, so `croniq-runner v0.4.0` ended up as the latest release and
  `/releases/latest` redirected to `…/tag/python-sdk-v0.4.0`. The installer
  derives the server version from exactly that redirect by cutting at the last
  `/v` — a tag with no `/v` in it leaves nothing to cut, so `CRONIQ_VERSION`
  became the whole URL and the download 404'd on a nonsensical path. Anyone
  running the documented `curl … | sh` one-liner in that window hit it.

  The Python and Go release workflows now pass `make_latest: false` (the other
  three SDKs publish to their registries and create no GitHub Release), and
  `install.sh` checks that what it resolved looks like a version before using
  it, so a redirect that lands somewhere unexpected fails with a message that
  says so. The v0.34.0 release was flipped back to "Latest" by hand.

- **The Python SDK's PyPI upload works again.** `pypa/gh-action-pypi-publish`
  was pinned to a pre-v1.14.2 commit, whose image carries Twine 6 — and Twine 6
  rejects the `Metadata-Version: 2.5` that current hatchling writes
  (`'2.5' is not a valid metadata version`). It surfaced on the
  `python-sdk-v0.4.0` tag: build and `twine check` passed, because that check
  installs the latest Twine from PyPI, and only the upload failed. Pinned to
  v1.14.2, the first release whose image ships Twine 7. Nothing was published,
  so the tag was re-run rather than re-cut.

## [0.34.0] - 2026-08-21

### Added

- **Runner SDKs budget consecutive authentication failures
  ([#473](https://github.com/nuetzliches/croniq/issues/473)).** All six SDKs —
  Rust, Go, Python, TypeScript, Java, .NET — gained a
  `max_consecutive_auth_failures` knob (default `3`) and a dedicated fatal
  error for a run of `401 Unauthorized` responses on `POST /v1/work/poll`.

  **Behaviour change.** A `401` was classified as transient, so a runner whose
  key was revoked retried it every poll interval forever. The credential is
  read once, at construction, and never re-read, so retrying could not clear
  it: the process stayed up, looked healthy, did nothing, and never exited
  non-zero — which meant no supervisor ever restarted it, and restarting is
  exactly what would have picked up the new key. That is the gap the rotation
  grace window of [#472](https://github.com/nuetzliches/croniq/pull/472)
  papered over rather than closed: the window buys propagation time, but at
  its end the same endless loop began.

  Unlike the `403` of #437, the first `401` is not fatal. Rotation hands over
  by installing the new key and giving the old one an expiry (#471), so dying
  on a single rejection would turn a narrow race around that handover into an
  outage. A streak of them is a credential that is simply gone.

  New conformance case `17-poll-401-auth-ceiling.yaml`, green in all five
  runner bindings, plus `max_consecutive_auth_failures` in the case schema.

- **The CLI can authenticate, and manage credentials
  ([#475](https://github.com/nuetzliches/croniq/issues/475)).** `--api-key`
  (env `CRONIQ_API_KEY`) and `--url` (env `CRONIQ_URL`) are global arguments,
  sent as `Authorization: ApiKey <key>`.

  `status`, `list-runners` and `trigger` previously issued naked requests and
  called `.json()` on whatever came back, so against a server with auth
  enabled `croniq trigger` — the documented way to fire a job by hand — failed
  with a serde decode error rather than saying the request was unauthorised.
  Non-2xx responses now map to a message: a 401 without a credential names the
  flag to use, a 401 with one points at revocation and the rotation grace
  window, and a server-supplied `message` (the 409 `env_managed` refusal among
  them) is surfaced verbatim.

  New subcommands, over HTTP so they work against Postgres and remote servers
  the way `croniq init` cannot:

  ```
  croniq api-clients list|create|update|delete
  croniq api-keys   list|create|revoke
  ```

  `croniq api-keys list` marks a key replaced by a rotation as `retiring` and
  shows when it dies; `croniq api-keys revoke` is the break-glass path for a
  leaked credential. `--json` on the listing and create commands makes them
  scriptable.

- **Scoped API clients declared in the environment
  ([#471](https://github.com/nuetzliches/croniq/issues/471)).** A deployment
  can now pin least-privilege machine credentials from configuration alone:

  ```
  CRONIQ_API_CLIENT_RUNNER_KEY=croniq_…
  CRONIQ_API_CLIENT_RUNNER_SCOPES=work:poll,work:ack,work:renew
  CRONIQ_API_CLIENT_PRODUCER_KEY=croniq_…
  CRONIQ_API_CLIENT_PRODUCER_SCOPES=jobs:trigger
  ```

  Boot the stack and both clients exist with those scopes. Previously the only
  credential a deployment could pin by value was the single `admin`-scoped
  `CRONIQ_INIT_API_KEY`, and anything narrower had to be created through the
  API after boot — which inverts the usual ordering (env is rendered first),
  is neither declarative nor idempotent, and requires copying a
  server-generated key back into the deployment. So deployments reused the
  admin key everywhere, which is the opposite of the scoping the API supports.

  `<NAME>` is `[A-Z0-9_]`, lowercased with `_` → `-`, so
  `CRONIQ_API_CLIENT_RUNNER_POLL_KEY` declares `runner-poll`. Every key
  variable takes the `_FILE` form. `CRONIQ_API_KEY` (+ `_SCOPES`) is the short
  form for the `default` client; `CRONIQ_INIT_API_KEY` and
  `CRONIQ_INIT_API_KEY_RECONCILE` keep working as deprecated aliases.

  Scopes are **required** for a named client — omitting them is a boot error,
  not an implicit `admin` — and an unknown scope fails the boot rather than
  producing a credential that authorises nothing and fails at first use.

  Named clients live under `CRONIQ_API_CLIENT_` rather than extending
  `CRONIQ_API_KEY_<NAME>` because the latter is ambiguous: with the key value
  carrying no attribute suffix, `CRONIQ_API_KEY_FOO_SCOPES` could be the
  scopes of `foo` or the key of `foo-scopes`, and it collides with the control
  variables (`CRONIQ_API_KEY_RECONCILE`, `…_ROTATION_GRACE`).

- **`managed_by` on API clients, and the API refuses to fight the
  environment.** A client the environment declares is stored with
  `managed_by: "env"`; `PUT`/`DELETE /v1/api-clients/{id}` and
  `POST /v1/api-keys` for it now return `409 env_managed` with a message
  naming the variable to edit instead. The dashboard shows an `env-managed`
  badge and disables those controls.

  Without this an edit made in the dashboard would survive until the next
  reconcile and then revert, with nothing connecting the two events.

  Ownership never moves silently: a client that already exists as
  `managed_by: "api"` — created in the dashboard, or seeded by
  `croniq init --api-key` — stays API-owned until an operator sets
  `CRONIQ_API_KEY_RECONCILE=1`. Upgrading a deployment whose client names
  collide with new declarations therefore changes nothing on its own.
  Migration `026_api_client_managed_by` defaults every existing row to `api`.

- **Credential reconcile on `SIGHUP` and `POST /v1/admin/reload-config`.**
  The environment of a running process cannot be changed from outside, so a
  direct env var only ever takes effect at boot — but a `_FILE`-backed value
  can be rewritten under a live process, which is what a Kubernetes Secret
  volume or a Vault sidecar does. Both explicit reload triggers now re-read
  the declarations, making a key rotatable without a restart.

  The reload response gained a `credentials` array (one entry per declaration,
  with `created` / `rotated` / `scopes_updated` / `adopted` / `unchanged` /
  `blocked`) and a `credentials_error` field, so a deployment with no
  dashboard can see the result instead of grepping logs. `?dry_run=true`
  reports the same block without writing.

  The `--watch` file watcher deliberately does **not** re-read credentials: it
  fires on every write, including the partial one a secret manager makes
  halfway through replacing a file.

- **`Scope::ALL` / `Scope::is_known`** in `croniq-auth`, for validating scope
  strings that arrive from outside the type system.

- **`GET /v1/api-keys?client_id=…` — list a client's keys
  ([#471](https://github.com/nuetzliches/croniq/issues/471)).** Metadata only
  (`key_id`, `key_prefix`, `created_at`, `expires_at`, `revoked_at`); the key
  hash is never returned. Revoked and expired rows are included, because the
  question the endpoint answers is "which credentials exist and which are
  still live?".

  There was previously no way to enumerate keys at all: the raw value is shown
  once at creation and the `key_id` needed to revoke one was only available in
  that same response. An API-only deployment could therefore neither audit its
  credentials nor revoke a specific key after the fact. Requires
  `api-keys:admin`, bounded by the caller's own scopes; an unknown `client_id`
  is `404` rather than an empty list.

- **`CRONIQ_API_KEY_ROTATION_GRACE` — a handover window on key rotation
  ([#471](https://github.com/nuetzliches/croniq/issues/471)).** When
  `CRONIQ_INIT_API_KEY_RECONCILE=1` rotates the `default` client's key, the
  superseded key is now stamped with `expires_at = now + grace` (default
  `15m`) instead of being revoked outright. It keeps authenticating until the
  deadline, which is visible on the new listing endpoint.

  **Behaviour change.** A boot rotation previously revoked every active key the
  instant the new one was installed. That is a hard cut for every credential
  holder outside the server process: a runner in another container still has
  the old value in memory, and the runner SDK classifies `401` as transient
  (`ClientError::Server`), so it retries every few seconds indefinitely rather
  than exiting for its orchestrator to restart it. Instant revocation did not
  produce a brief blip — it produced a runner that never recovered on its own.
  The grace window covers a Kubernetes secret-volume refresh plus a consumer
  rollout.

  Set `CRONIQ_API_KEY_ROTATION_GRACE=0s` to restore the previous behaviour.
  The window is deliberately *not* the answer to a leaked key: to end one
  immediately, take its `key_id` from `GET /v1/api-keys` and call
  `DELETE /v1/api-keys/{id}`, or rotate with the grace set to `0s`. A
  malformed duration fails the boot rather than falling back to a window the
  operator did not choose, and anything over 24h logs a warning.

  New store method `AuthStore::set_api_key_expiry` backs this (SQLite,
  PostgreSQL, and the Postgres actor handle). It is a plain setter, like
  `revoke_api_key`; the rotation path owns the rule that an existing, earlier
  deadline is never pushed further out, so rotating repeatedly inside one
  window cannot keep the oldest key alive.

### Changed

- The 409 refusal on an env-declared client now states the consequence that
  actually applies to the attempted operation. Minting a key was described as
  being "reverted", when what happens is that the reconciler retires it —
  close enough to sound right and wrong enough to mislead.

- **`CRONIQ_API_KEY_RECONCILE=1` gates changes, not creation.** Creating a
  declared client that does not exist yet is additive — it cannot break a
  working credential — so it happens without the flag. Rotating a key,
  updating scopes and taking ownership all still require it. This is what lets
  "render the env, boot the stack, get two scoped clients" work in one step.

- A client that matches its declaration but is still API-owned is logged at
  `info`, not `warn`. Nothing is broken in that state — the row is merely
  still editable through the API — and warning about it on every boot would
  train operators to ignore the line that does mean something.

### Fixed

- **A declared key that is already another client's live credential is
  reported, not installed
  ([#522](https://github.com/nuetzliches/croniq/issues/522)).**
  [#520](https://github.com/nuetzliches/croniq/issues/520) refuses two
  *declarations* carrying one key value, which settles it when both sides come
  from the environment. It cannot see the other half: the colliding row already
  in the store. `croniq init --api-key` writes one, and so does a client the
  environment has stopped declaring — a reconcile never touches an undeclared
  client, so its key stays live indefinitely. Renaming a declared client by
  editing `CRONIQ_API_CLIENT_PRODUCER_*` into `CRONIQ_API_CLIENT_TRIGGER_*`,
  key value and all, is exactly that shape.

  The reconciler used to install the declaration anyway, landing in the state
  #520 describes from the other direction: two `api_keys` rows, one `key_hash`,
  two clients. The credential then authenticates as whichever row the lookup
  ranks first and carries only that client's scopes — #516 made the winner
  stable rather than correct — and the loser is a client that exists, is
  active, holds the scopes it was declared with, and `403`s.

  Such a declaration now reports the new outcome `conflicted`, names the client
  that already holds the value, and writes nothing: no client created, no key
  rotated, no scopes or ownership changed. Half a declaration would be worse
  than none — a client whose scopes came from the environment and whose
  credential answers as someone else — and for a client that does not exist
  there is nothing to create that could work, since `managed_by: "env"` also
  means `POST /v1/api-keys` refuses to mint it a key of its own.

  Reported rather than fatal, and reported on every pass. The colliding row is
  stored state, so failing the boot would take a server down over a mistake
  made on an earlier day — the objection that ruled out a unique constraint on
  `key_hash` in #516. And an already-collided pair needs no write, so
  `unchanged` was the reconciler's previous answer to the one state only it can
  see. `CRONIQ_API_KEY_RECONCILE=1` makes no difference either way: there is no
  write to gate.

  Only rows that could answer a request count as a collision. A revoked row is
  audit history and a lapsed `expires_at` is the tail of a finished rotation —
  neither ever resolves — so ending a key on one client and declaring its value
  on another still works, which is the fix the message itself suggests. A key
  still inside its rotation grace does count: that window exists precisely
  because the key is still in use.

  Not a gap: `POST /v1/api-keys` cannot be made to collide, deliberately or
  otherwise. It takes a `client_id` and nothing else, and mints the value
  itself.

- **The same key value declared for two API clients is refused at boot
  ([#520](https://github.com/nuetzliches/croniq/issues/520)).**
  `parse_declarations` already guarded two variables naming one client with
  different values; nothing guarded the mirror case — two clients named with
  one value:

  ```
  CRONIQ_API_CLIENT_PRODUCER_KEY=croniq_shared
  CRONIQ_API_CLIENT_PRODUCER_SCOPES=jobs:trigger
  CRONIQ_API_CLIENT_RUNNER_KEY=croniq_shared
  CRONIQ_API_CLIENT_RUNNER_SCOPES=work:poll
  ```

  Both declarations reconciled independently: two clients created, two
  `api_keys` rows carrying the same `key_hash`. Keys resolve by hash, so the
  credential authenticated as exactly one of them and carried only that
  client's scopes — and which one was nobody's decision. Both rows are
  un-revoked, open-ended and share a `created_at`, so
  [#516](https://github.com/nuetzliches/croniq/issues/516)'s ordering had no
  tie to break and the query plan decided. The losing client exists, is active,
  has the scopes it was declared with, and `403`s on its own endpoints with
  nothing in the reconcile output hinting why.

  The declaration is now an error at parse time, naming both variables and both
  clients, so nothing is written and no live credential has to be taken away
  from one side. #516 could not have caught this: both of its fixes are
  per-client, and no ordering repairs a secret that legitimately matches two
  identities.

  `CRONIQ_API_KEY` without `CRONIQ_API_KEY_SCOPES` is still the credential the
  CLI presents rather than a declaration
  ([#502](https://github.com/nuetzliches/croniq/issues/502)), so exporting a
  client's own key on the server host to run `croniq` there is unaffected.
  A key pasted in from a client created through `POST /v1/api-keys` is not
  covered — only one side of that collision is in the environment.

- **A re-declared key that had been revoked is restored, not duplicated
  ([#516](https://github.com/nuetzliches/croniq/issues/516)).**
  `api_keys.key_hash` carries a plain index, not a unique one — a revoked row is
  kept for audit — and `find_api_key_by_hash` selected without an `ORDER BY`. So
  once two rows held one secret, authentication answered from whichever row the
  query planner happened to return, and the same credential was accepted or
  rejected depending on the plan.

  `sync_declared_client` produced exactly that pair. A revoked row did not
  satisfy the declaration, so re-declaring a revoked key — a rotation rolled
  back under `CRONIQ_API_KEY_ROTATION_GRACE=0s`, or one where the outgoing key
  was ended with `DELETE /v1/api-keys/{id}`, which is the documented answer to a
  leak — minted a *second* row for the same secret beside the revoked one.
  [#500](https://github.com/nuetzliches/croniq/issues/500) closed the same hole
  for a merely *dated* row; this was the other half.

  The declared row is now restored instead. `restore_api_key` clears
  `revoked_at` and `expires_at` in one statement — a row can be dated *and*
  revoked, and half a restore is still a key that stops working — so the key
  keeps its `key_id` and there stays one row per secret. The restore then
  retires the key it supersedes, exactly as re-minting did: leaving that out
  would have made a rollback the one way to end up with two live keys.
  `CRONIQ_API_KEY_RECONCILE=1` still gates the write, and the blocked outcome
  now names the restore, so an operator who revoked a value the environment
  still declares reads that instead of watching it reappear.

  `find_api_key_by_hash` is ordered regardless of which path wrote the rows —
  un-revoked before revoked, open-ended before dated, latest deadline, newest
  row — because databases written before this fix already hold duplicates. The
  reconciler ranks candidate rows the same way, so it decides about the row the
  auth path will actually use.

  Deliberately no unique constraint on `key_hash`. It would have to be a
  migration that collapses existing duplicates, i.e. deletes audit rows, and it
  would turn one config mistake — the same key value declared for two clients —
  from a reportable outcome into a write that fails at boot. The ordering makes
  the duplicates that exist harmless; the reconciler no longer makes new ones.

- **Four SDKs now actually reset their poll-loop budgets
  ([#507](https://github.com/nuetzliches/croniq/issues/507),
  [#508](https://github.com/nuetzliches/croniq/issues/508)).** The
  `max_consecutive_auth_failures` note above states that the counter resets on
  a successful poll, and the conflict budget is documented as clearing on any
  non-409 failure. Rust and .NET did both; TypeScript, Go, Python and Java did
  neither:

  - The auth counter survived a successful poll, so a runner that took two
    401s during a rotation race and then polled cleanly for days was killed by
    the next isolated 401 — a lifetime allowance rather than a streak
    detector, which inverts what the budget is for.
  - The new 401 branch returned before reaching the conflict reset, so
    `409, 409, 401, 409` counted as three consecutive conflicts and stopped the
    runner with a conflict error naming a cause that was not the case.

  Case `17-poll-401-auth-ceiling` returns 401 forever, so it never exercised
  either reset — which is why five green bindings meant nothing here. Two
  cases now do: `18-poll-401-budget-resets-on-success` and
  `19-poll-401-clears-conflict-streak`. Both were checked to *fail* against the
  unfixed code, and both end their script with a `500` rather than an empty
  `200`, because a runner loops with no delay on the latter and would satisfy
  any request-count assertion whether the budget reset or not.

- **A removed job no longer reports `croniq_job_overdue` forever
  ([#470](https://github.com/nuetzliches/croniq/issues/470)).** The per-job
  liveness series are now emitted only for jobs the running configuration
  defines.

  Their source, `job_states`, outlives the job that created it — nothing
  deletes a row — and the exporter read straight from the table. So a job
  removed from the Croniqfile months earlier kept emitting
  `croniq_job_overdue{job_key="demo:smoke"} 1` with a `next_fire_at` far in
  the past, which defeats exactly what the metric is for: anyone following the
  documented `croniq_job_overdue == 1` alert got a permanent false positive
  they could only clear with direct SQL against a stopped server. Emitting a
  series for a job the scheduler does not know about is wrong independently of
  whether the row is ever deleted, so the fix is in the exporter rather than in
  a retention policy. A server with no trigger map to consult still emits
  everything — "cannot tell" must not become "emit nothing".

  The rows are deliberately still kept: a job commented out for a week should
  keep its state, and the loader cannot tell "removed" from "temporarily
  absent". It now logs them once at startup, naming the keys, instead of
  leaving them invisible.

- **`DELETE /v1/jobs/{job_key}` clears the job's `job_states` row.** It
  deleted from `trigger_definitions` and `job_definitions` only, so the
  supported deletion route left the scheduling state behind — and with it a
  stale `last_fired_at` / `fire_count` that would resurface if the key were
  ever reused. Best-effort and logged on failure: the definition is already
  gone at that point, and failing the request would report a delete that half
  happened as no delete at all.

- **`dead` executions are reachable by retention when nothing references them
  ([#470](https://github.com/nuetzliches/croniq/issues/470)).** Both
  `server { execution_retention }` and `keep_last` filtered `state <> 'dead'`
  outright, on the documented grounds that per-job `dead_letter { retention }`
  governs them instead. But that retention only ever deletes from
  `dead_letters`; it never touches `executions`. A dead execution that never
  produced a letter, or whose letter had already been purged, therefore had no
  governing retention at all and accumulated forever — the unbounded-history
  growth #344 was introduced to close, still open for one state.

  Both paths now include a `dead` execution when no `dead_letters` row
  references it. One that has a letter is still left to dead-letter retention,
  so the documented split is unchanged where it actually applied.

- **`SIGHUP` no longer rotates credentials when the Croniqfile it accompanies
  is rejected ([#480](https://github.com/nuetzliches/croniq/issues/480)).**
  The signal handler reconciled the environment-declared API clients *before*
  validating the new config, so an operator who edited a key and a schedule
  together and got a syntax error in the schedule still had the old key
  retired: the disruptive half of the reload landed, the half that was supposed
  to accompany it did not. `POST /v1/admin/reload-config` already deliberately
  waits for a successful apply; both triggers now agree. A rejected reload logs
  that the credential half was skipped.

- **`croniq api-keys list` distinguishes an expired key from a retiring one
  ([#483](https://github.com/nuetzliches/croniq/issues/483)).** The `STATE`
  column derived `retiring` from the mere presence of `expires_at`, so a key
  whose grace window had elapsed hours earlier — one the server had been
  answering `401` for ever since — still read as mid-handover, directly beside
  the past timestamp that said otherwise. It now reports `expired` once the
  deadline has passed, using the same strictly-later comparison the auth
  middleware enforces so the two never disagree about the deadline instant
  itself, and the trailing hint explains that state instead of advising a
  revoke nobody needs.

- **A removed job stops being reported by the states API, the watchdog and the
  MCP tool too ([#506](https://github.com/nuetzliches/croniq/issues/506)).**
  #470 stopped the *metrics exporter* emitting series for jobs the
  configuration no longer defines. Three other consumers of the same
  never-deleted `job_states` rows kept doing it:

  - `GET /v1/jobs/states` computed `overdue` from the stored row, so the
    dashboard badged a job removed months ago as permanently stalled.
  - The watchdog's missed-fire sweep dispatched `job_missed_fire` alerts for
    it, and again after every restart — the dedup set is in-memory and
    `next_fire_at` never advances, so it paged someone repeatedly about a job
    that does not exist.
  - The MCP `list_jobs` tool handed an agent the rows verbatim.

  The rule now lives in one type, `LiveJobs`, rather than being restated per
  consumer — chiefly so its fail-open half cannot be got backwards: "cannot
  tell which jobs are live" has to mean "report everything", never "report
  nothing". The watchdog gets the same trigger snapshot the exporter reads,
  not its boot-time DSL job list, since filtering on that would have silenced
  missed-fire alerts for every job registered through the API.

- **Jobs added or removed through the API keep their `croniq_job_*` series in
  step ([#505](https://github.com/nuetzliches/croniq/issues/505)).** The
  phantom-job filter of #470 decides which jobs may emit per-job metrics from
  the shared trigger snapshot — but that snapshot was written only at boot and
  on reload, while `POST /v1/jobs/register` and the other API mutation routes
  reached the scheduler's own map alone. A job registered by a runner at
  startup therefore had *no* `croniq_job_overdue`, `last_fire` or `next_fire`
  series until the next reload, which on a server without `--watch` or a
  `SIGHUP` means indefinitely: an operator's documented
  `croniq_job_overdue == 1` alert silently covered none of their dynamically
  registered jobs. The inverse of the false positive #470 removed.

  The snapshot is now synced where every runtime command already converges, in
  the scheduler task, rather than at each of the six routes that can send one.

- **A store error no longer lets `DELETE /v1/api-clients/{id}` through
  ([#504](https://github.com/nuetzliches/croniq/issues/504)).** The env-managed
  refusal was reached via `if let Ok(Some(client))`, so a transient lock or IO
  failure on the lookup took the same branch as "no such client": the guard was
  skipped, the delete went ahead, and the handler answered `204` — for an
  env-owned credential that only a boot or reload would restore. The lookup and
  the delete now both surface their failure as a `500`, while a genuinely
  absent client stays the `204` it always was.

- **An upgrade no longer fails to boot over env values v0.33.0 tolerated
  ([#503](https://github.com/nuetzliches/croniq/issues/503)).** Two inputs
  became fatal where they had been ignored, so a server that had been running
  fine refused to start after the version bump — and since the scheduler *is*
  the process, every job stopped until someone edited the environment:

  - A `CRONIQ_INIT_API_KEY` that is not a `croniq_` key, e.g. a `changeme`
    left in an old template. v0.33.0 logged "env value ignored" and booted; the
    deprecated spelling does so again.
  - A `CRONIQ_API_CLIENT_*` variable with an unrecognised suffix. Croniq never
    reads those, so refusing to start claimed a whole environment namespace on
    the strength of a variable it has no use for.

  Both are now logged and skipped. Everything that is an actual declaration
  written wrong — a malformed key in a *current* variable, a named client
  without scopes, an unknown scope, one client declared twice — is still fatal,
  because booting past one means running without the credential that was asked
  for. The typo that matters, a misspelled `_SCOPES`, still fails loudly via
  the missing-scopes check.

- **A stale key no longer resurrects a deleted API client
  ([#499](https://github.com/nuetzliches/croniq/issues/499)).** Creating a
  declared client was ungated on the reasoning that it is additive and cannot
  break a working credential. That holds for a client which never existed; it
  does not hold for one an operator deliberately removed. Delete the `default`
  client after a key leak, leave `CRONIQ_INIT_API_KEY=<leaked>` in the
  deployment, and the next boot recreated it — active, `admin`-scoped, keyed
  with the leaked value — where v0.33.0 had logged "only seeds on fresh
  `croniq init`" and done nothing. The recreated row is `managed_by=env`, so
  `DELETE /v1/api-clients/{id}` then answered 409 and the remediation could not
  be repeated through the API at all.

  A declaration now has to say what the client is *for* before it will create
  one: a key with no scopes named rotates an existing client and reports
  `skipped` when there is none. Naming scopes is an unambiguous statement that
  the client should exist, so the one-step "render the env, boot the stack"
  flow is untouched, and `croniq init --api-key` still seeds on a fresh data
  dir — the first-run path does not go through the reconciler.

- **Rolling a key rotation back no longer leaves the key dying on a timer
  ([#500](https://github.com/nuetzliches/croniq/issues/500)).** `needs_key`
  asked only whether a matching key row was un-revoked, never whether it was
  *expiring*. So after rotating A→B and rolling back to A, the reconciler saw
  its declared key present and reported `unchanged` — while A still carried the
  `expires_at` the rotation had stamped on it. Nothing clears one: the store's
  setter took a non-optional timestamp, and a fresh row is only minted when
  `needs_key` is true. Once the grace window elapsed every consumer of the
  declared credential got `401`, permanently, with every reconcile still
  reporting `unchanged`; the only way out was revoking the row by hand so a new
  one would be minted.

  A declared key that is mid-retirement now has its deadline cleared rather
  than a second row minted with the same secret — `api_keys.key_hash` is not
  unique, so a duplicate would leave authentication choosing between the two
  (the lookup is ordered as of #516). Without
  `CRONIQ_API_KEY_RECONCILE=1` nothing is written, but the outcome now says
  what is pending instead of claiming nothing is.

- **`CRONIQ_API_KEY` declares a client only when its scopes are named
  ([#502](https://github.com/nuetzliches/croniq/issues/502)).** The variable has
  two meanings: to the CLI and the SDKs it is the credential to *present* —
  `croniq --api-key` reads it, and the docs tell operators to export it — while
  to the server, new in this cycle, it declared an `admin`-scoped `default`
  client. So exporting a deliberately narrow key on the server host, in order
  to run `croniq` there, created an admin client keyed with it; because keys
  resolve by hash, that same narrow key then authenticated as admin. Creation
  is not gated by `CRONIQ_API_KEY_RECONCILE`, so nothing had to be opted into,
  and v0.33.0 ignored the variable entirely.

  Setting `CRONIQ_API_KEY_SCOPES` is now what makes `CRONIQ_API_KEY` a
  declaration; without it the value is logged as a client credential and
  skipped. The deprecated `CRONIQ_INIT_API_KEY` keeps declaring an `admin`
  client on its own — it has never been a client-side variable, and the demo
  stack and existing deployments rely on it — and setting both spellings still
  declares, so a migration in progress is unaffected.

- **An unset `CRONIQ_API_KEY_SCOPES` no longer re-escalates a narrowed client
  ([#501](https://github.com/nuetzliches/croniq/issues/501)).** v0.33.0's
  reconciler only ever rotated keys. This cycle's also force-syncs scopes — and
  the declaration stored the *implied* admin of a bare `CRONIQ_API_KEY` /
  `CRONIQ_INIT_API_KEY` as if the operator had asked for it. So a `default`
  client narrowed to `jobs:trigger` in the dashboard looked like scope drift,
  and the first boot with the documented legacy rotation pair set put it back
  to full admin, in the granting direction, with a single `warn` as the only
  trace.

  "The environment named no scopes" and "the environment asked for admin" are
  now distinct: a client that does not exist yet is still created with `admin`
  (the back-compat the bare variable has always had), an existing one keeps
  the scopes it has, and an adoption moves ownership without rewriting them.
  Naming scopes explicitly syncs them exactly as before.

- **The `env_managed` refusal names the variable that actually declares the
  client ([#481](https://github.com/nuetzliches/croniq/issues/481)).** Both the
  409 body and the dashboard's hint rebuilt the name as
  `CRONIQ_API_CLIENT_<NAME>_KEY` for every env-owned client. For `default` —
  declared by `CRONIQ_API_KEY`, outside that namespace — the result was
  `CRONIQ_API_CLIENT_DEFAULT_KEY`, a *second* declaration of the same client.
  So an operator who followed the advice did not merely fail to make the edit
  land; they armed a fatal both-declare error for the server's next boot. The
  server now resolves the name against its live environment, which also lets it
  name the deprecated `CRONIQ_INIT_API_KEY` alias when that is what is set.

- **The retention sweep no longer scans `dead_letters` once per candidate row
  ([#485](https://github.com/nuetzliches/croniq/issues/485)).** Reaching
  unreferenced `dead` executions (#470) added a correlated
  `NOT EXISTS (… WHERE dl.execution_id = e.id)` probe to both prune paths, but
  `dead_letters` was indexed by `job_key` and `expires_at` only — never by the
  column that probe joins on. So every 30 s watchdog tick paid a full scan of
  the dead-letter table per candidate execution, worst on exactly the
  installations with a large backlog. Migration `027` adds the index, in both
  backends.

  The predicate itself was copy-pasted eight times — four queries per backend,
  in two dialects, with the comment explaining the rule present in only one of
  them ([#488](https://github.com/nuetzliches/croniq/issues/488)). It is now
  one shared fragment.

- **An out-of-range `CRONIQ_API_KEY_ROTATION_GRACE` is reported, not acted on
  ([#482](https://github.com/nuetzliches/croniq/issues/482)).** The parsed
  second count reached `chrono::Duration::seconds` through a bare `as i64`
  cast. Above `i64::MAX` seconds that wrapped negative, and a negative grace
  makes `now + grace` a moment in the past — so a mistyped value produced the
  exact opposite of the knob's purpose, revoking every superseded key on the
  spot. Between the two limits it panicked at boot instead. Both are now a
  boot error naming the variable.

- **One duration grammar for every knob
  ([#486](https://github.com/nuetzliches/croniq/issues/486)).**
  `CRONIQ_API_KEY_ROTATION_GRACE=1d` was a fatal boot error while
  `server { execution_retention 30d }` a few lines away in the same file was
  fine: the rotation grace went through a second, narrower parser
  (`s`/`m`/`h`/bare) than the `ms`/`s`/`m`/`h`/`d` one every other duration
  uses. Two grammars for the same-looking value is a trap regardless of which
  one is "right", so there is now one — `parse_duration_checked` in
  `croniq-execution` — and the server's seconds-granularity settings adapt it
  rather than reimplement it.

  The shared parser also multiplied its unit factor unchecked, so a large
  `<n>d` wrapped (or panicked in a debug build) instead of being rejected; it
  now reports the overflow. Settings measured in whole seconds — lease TTLs,
  the dedup window, the rotation grace — reject a sub-second value instead of
  truncating `500ms` into a zero-second lease.

- **CI is green on Rust 1.98.** Clippy 1.98 tightened `result_large_err`, which
  now fires on the five `auth_endpoints` handlers returning
  `Result<T, Response>` — all three `-D warnings` clippy gates failed on `main`
  the moment the toolchain rolled from 1.97.1 to 1.98.0, with no code change
  involved. Those handlers return a `Response` because they attach headers on
  failure (clearing the refresh cookie, throttling headers) and a bare
  `StatusCode` cannot carry those; boxing it as clippy suggests would mean
  `Box::new` at 22 error sites to move 128 bytes off a path that already
  allocates a response body. Allowed for that module, with the reasoning
  recorded at the top of the file.

## [0.33.0] - 2026-08-19

### Added

- **Go SDK: a ceiling on consecutive poll conflicts
  ([#466](https://github.com/nuetzliches/croniq/issues/466)).** New
  `WithMaxConsecutivePollConflicts` / `Options.MaxConsecutivePollConflicts`
  (default `3`) budgets consecutive `409 Conflict` responses to
  `POST /v1/work/poll`. On exhaustion `Runner.Run` returns the new
  `*PollInstanceConflictError`, which carries `RunnerID` and
  `ConsecutiveCount` and names the remedy: stop the duplicate process or
  rotate the `runner_id`. The counter resets on a successful poll or on any
  non-409 failure (5xx, network, timeout), which say nothing about instance
  ownership.

  **Behaviour change.** A sustained `409` previously retried forever. One
  conflict is still transient — a deposed instance may win its identity back,
  and conformance case 11 pins that it is retried — but a *streak* of them is
  a duplicate deployment, two processes started with the same fixed
  `runner_id`, and retrying that forever left the misconfiguration behind a
  warning that scrolled past. The runner now exits so the process can fail
  non-zero and reach monitoring. Set the option to `100` to get close to the
  old behaviour.

  The same option landed in the Python, TypeScript and Java SDKs in this
  release (see their changelogs); the Rust and .NET SDKs have had it since
  [#134](https://github.com/nuetzliches/croniq/issues/134) sub-item 1, and
  that asymmetry is what #466 closes. The `403` half was already symmetric
  (#437/#458).

- **Conformance case `16-poll-409-conflict-ceiling.yaml`.** A server answering
  `409` on every poll with `max_consecutive_poll_conflicts: 2`, and a
  `max_count` on the poll expectation so a runner that retries forever fails.
  `runner_config.max_consecutive_poll_conflicts` had been in
  `schema/case-schema.json` since #134 while only .NET implemented it, so no
  case could use it: as of #460/#467 the other four bindings reject a
  schema-legal key they have not implemented, which is the correct behaviour
  and also meant the corpus could not pin this contract until they had the
  option. All five bindings now accept the key and pass the case — the first
  corpus coverage the .NET implementation has had.

- **The dashboard refreshes expired access tokens instead of signing you out.**
  A 401 mid-session now triggers one refresh and one retry of the original
  request; only a refresh that finds no session ends it. Access tokens live an
  hour against a seven-day refresh token, so the previous behaviour dropped
  users at the login screen hourly. Reloads recover the session from the
  refresh cookie, which is why they briefly show a spinner rather than the
  login page.

### Changed

- **A cross-origin dashboard build must acknowledge its weaker token storage.**
  A `SameSite=Strict` cookie cannot reach a dashboard served from a different
  origin than the API, so a `VITE_API_URL` build necessarily keeps the refresh
  token in `localStorage`. `ui/vite.config.ts` now refuses such a build unless
  `VITE_ALLOW_LOCALSTORAGE_REFRESH=1` is set alongside it, so nobody lands in
  that mode as a side effect of pointing the UI at another host. Local
  development is unaffected — `npm run dev` proxies `/v1` through the Vite dev
  server and is therefore same-origin. See `docs/operations.md` → *Where the
  dashboard keeps its tokens*.

### Fixed

- **SSO login no longer ends on a page of raw JSON.** The IdP redirects the
  browser to `GET /v1/auth/oidc/callback`, which answered with a
  `TokenResponse` body — leaving the user looking at JSON while the dashboard
  never received the tokens, and `oidc.post_login_redirect` parsed but never
  used. A browser navigation now gets the refresh cookie and a 302 to
  `post_login_redirect`; a caller that explicitly asks for
  `Accept: application/json` still gets the JSON body. No token ever appears in
  a URL, in browser history, or in a proxy log.


- **An API client's refresh token was minted but never stored, so it could not
  be redeemed** (#463). `POST /v1/api-clients/{id}/tokens` returned a full
  access/refresh pair without writing the `refresh_tokens` row that
  `POST /v1/auth/refresh` looks the presented token up in, so a machine caller
  that saved the token got a 401 the first time it tried to use it — while
  `openapi.yaml` advertised the field without qualification. The endpoint now
  persists the refresh half the way the login and OIDC paths do, and fails the
  request outright if that write fails rather than handing back a credential
  that cannot work.

  This also brings the refresh handler's API-key branch to life for the first
  time: it is reachable only from a `refresh_tokens` row with no `user_id`, and
  nothing had ever created one. Since it had never run, it was missing the
  checks its user-side counterpart has, and gained them here — a refresh now
  re-resolves the owning client on every rotation, so a narrowed scope list
  takes effect on the next refresh, a deactivated client gets a 403, and a
  deleted one gets a 401 instead of rotating into a scope-less token.

  Nothing in the repo consumed the field, so no client behaviour changes; a
  machine credential simply gains the token renewal the endpoint always claimed
  to offer, without needing the issuing admin credential again.

- **`calendar { timezone … }` was accepted, validated, compiled, persisted and
  displayed — and then ignored.** No evaluator ever read it. `Trigger::gate_allows`
  localized calendar gates with the *job's* zone, so a calendar declaring its
  own zone parsed, passed `validate`, survived `compile`, round-tripped through
  `GET /v1/calendars`, showed up in the dashboard, and had no effect whatsoever
  on when the gate opened. #449 deliberately shipped no diagnostic for it
  because the field deserved to work rather than be forbidden; this is that
  work (#450).

  A calendar's rules — `weekly`, `monthly`, `annual`, `dates` **and** `window`
  — are now evaluated on the calendar's own clock, resolved
  `calendar { timezone … }` > `defaults { timezone … }` > UTC. The consulting
  job's zone is deliberately **not** in that chain: a calendar is a named,
  shared resource, so "this holiday calendar is Austrian" has to hold for every
  job that references it. With the job's zone as the fallback, one calendar
  object would denote a different set of instants per consumer, and neither
  `GET /v1/calendars` nor the calendars page could answer "which zone is this
  calendar in?". A job's own times — its wall-clock schedule, its `window`
  directive, `not_before` / `not_after` — keep using the job's zone, unchanged.

  So a New York job firing at 22:00 against a Vienna calendar is asking about
  the *Vienna* day, which at that hour is already tomorrow: Friday 22:00 in New
  York is Saturday 04:00 in Vienna and the gate stays shut. Each zone follows
  its own DST switch too — the two are three weeks apart in spring, and in
  those weeks the same job time lands an hour differently on the calendar's
  clock.

  Gate advancement (#391's O(days-to-opening) jump, not a tick walk) had to
  change shape for this: with the calendar and the trigger `window` on
  different clocks there is no single local timeline to intersect interval sets
  on, since a Vienna 08:00..18:00 projected into New York is not the same
  second-of-day set on every day. Each gate now reports its next opening in its
  own zone and `next_gate_open` advances to the later of the two and re-asks,
  which is the same answer the old intersection gave when the zones coincide.
  `Calendar::allowed_intervals_on`'s date/time factoring stays valid and stays
  pinned — read from inside the calendar's own zone it was always the right
  model. One subtlety is now handled explicitly: resolving a local opening
  during a fall-back's repeated hour picks the occurrence at or after the scan's
  start, because the earlier one would move the scan backwards and stall it.

  Two smaller gaps closed alongside it:

  - `POST` / `PUT /v1/calendars` never validated `timezone` — the one
    timezone-bearing column #426 did not reach. It now answers `400`
    `unknown_timezone` with the same did-you-mean. A row written before this
    check is logged at `WARN` and evaluated in UTC rather than failing the
    calendar, since under `strict_calendars` an error there would pause every
    job consulting it — a worse upgrade than running it in UTC out loud.
  - `croniq validate` now warns about a calendar that has rules but no zone
    from anywhere: *"its rules are interpreted as UTC, not in the zone of the
    jobs that consult it"*. This is the half of #427 that was left unbuilt —
    the `has_timezone` hook that issue computed and discarded was measuring the
    wrong thing only because the runtime ignored the field. A warning, never an
    error; `Croniqfile.example` declares a zone on its calendar, so it stays
    silent there.

  `croniq compile` now prints the **effective** calendar zone (a calendar
  inheriting `defaults { timezone Europe/Vienna }` reports it instead of
  `null`), and the calendars page shows each calendar's effective zone, `UTC`
  when unset. A calendar created through the API is not part of any Croniqfile
  and so does not inherit that file's `defaults { }` — it falls back to UTC,
  which keeps its meaning from shifting when an unrelated file changes.

  Upgrade note: for a deployment where a calendar declares a zone that differs
  from the zone of a job consulting it, this **changes when that job fires** —
  the field now does what it says. Deployments whose calendars declare no zone,
  or where calendar and job zones agree (the usual single-zone Croniqfile), are
  unaffected.

- **The Java conformance suite ran a hardcoded subset of the shared case
  corpus, and silently.** Both Java suites filtered
  `sdks/conformance/cases/` and `sdks/conformance/cases-trigger/` through a
  `SCOPE` allowlist of filenames. A case not named in that set was not run at
  all: no skip was reported, no test appeared in the report, the suite was
  green and CI was green. The allowlist made sense while the Java SDK was
  being built out in stages, but the SDK has been feature-complete against the
  corpus for several releases and the default had the wrong polarity — the
  cost of forgetting was silence rather than a failure. That trap fired twice
  in the last release: cases 13/14 were added and implemented in all five SDKs
  while Java ran neither (#452), and case 15 repeated it (#458). Both were
  caught only by counting the cases by hand against the Gradle output, and
  both were fixed by appending filenames, which left the trap armed.

  The polarity is now inverted. Every YAML in the corpus runs by default, so a
  newly added case is picked up automatically and has to pass. The only escape
  hatch is an `UNSUPPORTED` map of `filename -> reason`, which is deliberately
  expensive to use: an excluded case is reported as a *skipped* test whose
  name carries the reason (Gradle's JUnit XML writer emits a bare `<skipped/>`
  element and drops the abort message, so the reason lives in the display name
  where every report format preserves it), and a dedicated guard test fails
  the suite if an `UNSUPPORTED` entry names a file that is no longer in the
  corpus, so the exclusion list cannot rot back into silence. An empty or
  missing corpus directory is now an error rather than a vacuous pass in both
  suites — previously the trigger suite emitted a green placeholder test when
  it found no cases, which would have hidden a mistyped path completely.
  `UNSUPPORTED` ships empty in both suites: nothing in either corpus is
  inapplicable to Java.

  This immediately surfaced one case the allowlist had been hiding since it
  was added: `04a-cancel-at-max-inflight-1.yaml`, the control-slot polling
  case from #176 that pins cancel delivery to a `max_inflight=1` runner that
  is already at capacity. It was never in `SCOPE` and had never run under
  Java. It passes — the Java SDK implements the contract correctly — but the
  suite had not been proving it. The Java runner suite now executes all 16
  runner cases and all 11 trigger cases.

  Audited the other four bindings for the same defect while here: .NET, Go,
  Python and TypeScript all enumerate the corpus dynamically
  (`Directory.EnumerateFiles`, `os.ReadDir`, `Path.glob`, `readdirSync`) with
  no allowlist anywhere, so all four already ran the full corpus including
  `04a`, and all four fail loudly rather than skipping when they meet a
  handler behavior they do not implement. All five bindings also genuinely
  enforce `max_count` ceilings: each suppresses the wait loop's early exit for
  any case carrying a ceiling assertion and burns the full `duration_max_ms`
  window, so a runner that overshoots after the lower bounds are met is still
  caught. Java was the only binding carrying a case-level allowlist.

- **Every conformance binding silently ignored assertion keys it had not
  implemented** (#460). None of the five bindings rejected a key its own case
  model did not cover: .NET built its YamlDotNet deserializer with
  `IgnoreUnmatchedProperties()`, Go used non-strict `yaml.Unmarshal`, Python and
  Java picked keys out of the parsed map by name, and TypeScript did a bare
  `load(text) as CaseSpec` with no runtime validation at all. An unrecognised
  key was dropped, the case loaded cleanly, and the assertion it carried simply
  was not there by the time the assertion loop ran — a green suite for an
  unenforced contract. This is the failure mode of the Java `SCOPE` allowlist
  (#453) one level down: that one skipped whole *cases* in silence, this one
  skipped individual *assertion keys*.

  All five bindings now fail at load time on a key they do not model, and each
  carries negative tests that provoke the silence at every level a case nests
  (top level, `runner_config`, handler, `server_script` entry, `respond`,
  `expectations`, HTTP expectation, and the trigger-side `request` / `expect` /
  `expect.response`) plus a positive counterweight so a broken fixture cannot
  make those tests pass for the wrong reason. Mechanically: .NET drops
  `IgnoreUnmatchedProperties()`, Go decodes with `KnownFields(true)`, and
  Python, TypeScript and Java assert each node's key set against the vocabulary
  the binding implements.

  This is deliberately **not** JSON-Schema validation inside the bindings, which
  is what the issue suggested. CI already validates the whole corpus against
  `schema/case-schema.json` and `schema/trigger-case-schema.json` — the
  `Conformance YAML schema` job runs `check-jsonschema` in all five SDK
  workflows — and that answers a different question. Schema validation catches a
  key the *schema* does not allow; it cannot catch a schema-legal key a
  *binding* has not implemented, which is precisely the hole #460 was filed for.
  Repeating the schema check in five languages would have added five
  dependencies and left that hole open. The two checks are complementary, and
  the per-binding key sets are expected to lag the schema wherever a capability
  is not universal: `runner_config.max_consecutive_poll_conflicts` is in the
  schema but only the .NET SDK has the option, so a case using it now fails
  loudly in the other four instead of running with it ignored.

  Strictness immediately surfaced a live instance in the Go binding, exactly the
  scenario the issue predicted. `body_absent` — the trigger-case assertion that
  pins the *omission* of unset optionals, so a producer cannot emit a
  `metadata` / `require` / `prefer` / `timeout` / `idempotency_key` field it was
  never given — existed in Go only as a comment in `trigger_spec.go`. Four
  trigger cases declare it (`01-trigger-minimal`, `03-trigger-metadata`,
  `04-trigger-require-prefer`, `05-trigger-timeout`); Go parsed them, dropped
  the key, asserted nothing, and reported green. Go now models and asserts
  `body_absent` against the first matching request, matching the .NET, Python,
  TypeScript and Java semantics, with a test that fails when a listed key is
  present. Java's `CaseLoader` also stopped carrying private copies of
  `loadRoot` and `parseScript` and now routes through `YamlSupport` alongside
  `TriggerCaseLoader`, so the YAML 1.1 `on:` workaround and the script-entry
  vocabulary exist once rather than twice — two copies of a key list being the
  drift this issue is about.

### Security

- **The dashboard's refresh token is out of `localStorage`** (#454). A password
  or SSO login now delivers it as a `croniq_refresh` cookie
  (`HttpOnly; Secure; SameSite=Strict; Path=/v1/auth`) and the access token
  lives in memory only, so neither an XSS nor a compromised npm dependency
  executing at runtime can read the credential that is good for seven days.
  `token_generation` (#431) had already narrowed the access token's blast
  radius, which left the refresh token as the more valuable of the two things
  sitting in `localStorage`.

  The CSRF-free property that motivated the previous design is intact:
  `SameSite=Strict` keeps the cookie off every cross-site request, only
  `POST /v1/auth/refresh` accepts it, and the token it mints lands in a
  response body a foreign page cannot read (CORS is origin-locked as of
  #429/#446). Every other API call still authenticates with an `Authorization`
  header.

  Mechanics worth knowing:

  * Cookie delivery is **opt-in per request** (`"refresh_cookie": true` on
    `/v1/auth/login`, `/v1/auth/login/totp` and
    `/v1/auth/login/enroll/totp/confirm`). Non-browser clients — the CLI, curl,
    the SDKs — are untouched and keep receiving `refresh_token` in the body.
  * A **cookie-sourced refresh never returns `refresh_token` in the body**.
    Without that rule the cookie would buy nothing: a script could POST to the
    refresh endpoint, have the browser attach the `HttpOnly` cookie for it, and
    read the rotated token out of the response.
  * `POST /v1/auth/refresh` and `/v1/auth/logout` now accept the token from the
    cookie when the body carries none; logout revokes it server-side and clears
    the cookie in the same response.
  * `Secure` is set only on positive evidence of HTTPS (`Origin`,
    `X-Forwarded-Proto`, or an `https://` `app_url`) — browsers refuse to send
    a `Secure` cookie over plain HTTP, so guessing would lock operators out
    rather than harden anything.

## [0.32.0] - 2026-08-18

### Added

- **Dependency advisory scanning in CI.** A new `cargo-deny` job in the CI
  workflow checks `Cargo.lock` against the RustSec advisory database and
  enforces the MIT-compatible-licenses rule from `AGENTS.md` on every PR that
  touches Rust files; policy lives in the new `deny.toml` at the repo root.
  The UI job gained an `npm audit --omit=dev --audit-level=high` gate for the
  production dependency tree. Because advisories appear without the code
  changing, a weekly scheduled workflow (`security-audit.yml`) additionally
  scans main's Rust tree and all three npm lockfiles (`ui/`,
  `sdks/typescript/`, `sdks/conformance/bindings/typescript/`) regardless of
  activity. One advisory is ignored with an inline justification:
  RUSTSEC-2023-0071 (Marvin timing sidechannel in `rsa`, via `jsonwebtoken`)
  has no patched release, and Croniq never decrypts attacker-supplied RSA
  ciphertext, which the oracle needs.

### Security

- **Runner SDKs require HTTPS for non-loopback base URLs.** All five runner
  SDKs (.NET, Java, Python, Go, TypeScript) defaulted their base URL to
  `http://localhost:4000` and validated nothing beyond parseability, so an
  operator who kept the documented URL shape and swapped in a real host
  shipped the runner's API key as a cleartext `Authorization` header on every
  poll — roughly every 35 seconds, for the lifetime of the runner, with no
  warning anywhere. Go, Python and TypeScript additionally honour `HTTP_PROXY`
  by default (`http.DefaultTransport`, httpx's `trust_env`, undici), so the
  key also traversed any configured proxy in the clear; and a plaintext base
  URL is the precondition that lets a purely on-path attacker inject a
  redirect or rewrite a poll response.

  Every SDK now validates the base URL where the options are constructed — not
  on the first request — so a misconfiguration fails fast. `https://` is
  always accepted; `http://` is accepted only when the host is loopback
  (`localhost`, `127.0.0.0/8`, `::1`, including the bracketed `[::1]` form),
  which keeps the `http://localhost:4000` quickstart path working untouched;
  any other scheme is rejected outright. A cleartext URL on a non-loopback
  host is refused with a message that names the URL, the reason, and the
  opt-in. That opt-in is explicit and per-SDK-idiomatic — .NET
  `AllowInsecureHttp` (option property, also bindable from
  `Croniq:Runner`/`Croniq:Client`), Java `allowInsecureHttp(true)` on the
  options builder (`croniq.runner.allow-insecure-http` in the Spring starter),
  Python `allow_insecure_http=True`, Go `croniq.WithInsecureHTTP()` (runner
  option and client/trigger-client builder method), TypeScript
  `allowInsecureHttp: true` — and when it is used, the SDK still emits one
  loud startup warning stating that the API key travels in cleartext and is
  readable by anyone on the network path, including HTTP proxies. Both the
  runner client and the producer-side trigger client are covered in every SDK
  that ships both. The Rust runner SDK (`croniq-runner-sdk`) has the same gap
  and is tracked separately.

- **Runner SDKs pass server-supplied identifiers as structured log fields, and
  validate them on ingest
  ([#441](https://github.com/nuetzliches/croniq/issues/441)).** Four of the five
  runner SDKs interpolated `job_key` and `execution_id` directly into log
  *messages* — a TypeScript template literal reading "handler for &lt;jobKey&gt;
  (execution &lt;executionId&gt;) threw", and its Python, .NET and Java
  equivalents. A value containing CRLF
  forged log records; one containing ANSI escapes reached the operator's
  terminal raw. The threat actor is a malicious or compromised Croniq server,
  but not only: in a multi-tenant deployment anyone who can name a job key in
  the Croniqfile controls a string that round-trips to every runner unchanged.
  Impact is audit-trail integrity and terminal spoofing, not code execution.
  Both identifiers now travel as structured fields in Python
  (`logging` `extra=`), TypeScript (the `fields` map), .NET (an `ILogger`
  scope) and Java (SLF4J `MDC`); Go, which already did this via `slog`
  attributes, is unchanged. Where the SDK renders text itself — the TypeScript
  console logger, which wrote `message` verbatim — control characters are now
  escaped before the write, mirroring what Go's built-in `slog` handlers do;
  where the SDK delegates to a host framework, that framework owns rendering
  and the SDK does not escape a second time.
- **Runner SDKs refuse work assignments whose identifiers carry control
  characters, closing an unbounded logger-namespace growth
  ([#441](https://github.com/nuetzliches/croniq/issues/441)).** Every SDK now
  validates `job_key` and `execution_id` on ingest, before either can reach a
  handler, a log record or a telemetry attribute. The `job_key` rule is a
  *denylist*, not an allowlist: it rejects the scalar values a terminal
  interprets rather than prints — C0 (`U+0000`–`U+001F`, covering NUL, CR, LF
  and the ESC that introduces every ANSI sequence), DEL (`U+007F`), and C1
  (`U+0080`–`U+009F`) — bounded to 256 scalar values, and accepts every other
  printable character in any script, interior spaces included. An allowlist
  built from the Croniqfile lexer's unquoted-identifier set would have been
  wrong: `parse_job_key` also accepts a `QuotedString` and then enforces only
  the "two or three colon-separated parts" rule, so `job "billing:monthly
  invoice" { … }` is legal DSL today and `POST /v1/jobs` constrains the key not
  at all — refusing such a key would strand a valid configuration. Execution
  ids, which the server only ever emits as v4 UUIDs, keep a narrow
  `a-z A-Z 0-9 - _ . :` charset bounded to 64 characters. Neither length bound
  existed server-side (both columns are plain `TEXT`), so they are the SDKs'
  own. What the runner does with a refused assignment depends on which field is
  at fault: an unsafe `execution_id` is what would address an ack or renew, so
  there is nothing safe to report and the assignment is dropped; an unsafe
  `job_key` with a valid `execution_id` is acked as a *failure* naming the
  offending field, so the operator gets a dead-lettered execution instead of one
  the stale-claim reaper requeues and every later poll refuses again. This also
  fixes the second-order problem the same values caused: the Python SDK built a
  logger name per job key (`logging.getLogger(f"croniq_runner.job.{job_key}")`)
  and the .NET SDK a logger category (`CreateLogger($"CroniqJob.{jobKey}")`).
  Both caches are permanent and unbounded, so a server delivering many distinct
  keys grew client memory without bound — reachable whenever a catch-all handler
  is registered, since that accepts any key — and let a server place its records
  under a namespace the operator had configured with `propagate=False`, evading
  log filtering. Validation bounds the charset but not the *number* of distinct
  keys, so both SDKs now use a single fixed logger with the job key attached as
  a field instead. Two new conformance cases pin the behaviour across every SDK:
  `13-hostile-identifiers-rejected.yaml` (a `job_key` carrying CRLF and ANSI
  escapes is never dispatched — not even to a catch-all default handler — and
  is acked as a failure, with no lease renewal) and
  `14-hostile-execution-id-dropped.yaml` (an unsafe `execution_id` produces no
  ack at all). In both, the poll loop keeps running.

- **Defence-in-depth: seven hardening gaps closed** (issue #431). None was
  exploitable in the shipped server as configured, but several were guarded by
  exactly one accident of the boot path, which is the reason to close them
  rather than rely on the guard holding.

  *The auth middleware fails closed.* When `ServerState.jwt_config` was `None`,
  `require_auth` injected a synthetic caller carrying the `admin` wildcard, so
  every `require_scope` check passed and the entire REST API plus `/mcp` served
  an anonymous request as admin. The only thing keeping that out of the shipped
  binary was `main.rs` always passing `Some`. It now answers `401` and inserts
  a scope-less context instead, so the safety property can be read off the
  middleware rather than emerging from one call site. `JwtConfig`'s `Default`
  impl — which carried the literal secret `croniq-dev-secret-change-me` — is
  gone in favour of `JwtConfig::new(secret)` and a `for_tests()` constructor
  that generates a random one, making "signed production tokens with a string
  published in this repository" unrepresentable.

  *Postgres connections negotiate TLS.* The driver was handed `NoTls`
  unconditionally, so against a remote database the connection password and
  every row the auth tables return — password hashes, wrapped TOTP secrets,
  API-key hashes — crossed the network in cleartext. The connector is now
  rustls-based (`tokio-postgres-rustls`; no OpenSSL, no C toolchain), with
  libpq-style modes resolved from `sslmode=` in the connection string, then
  `CRONIQ_PG_SSLMODE`, then a default of `require` for a remote host and
  `prefer` for loopback or a unix socket. Certificate verification is always on
  when TLS is used — Croniq's `require` behaves like libpq's `verify-full` —
  with roots from the platform trust store, the Mozilla bundle, and an optional
  PEM file named by `CRONIQ_PG_ROOT_CERT`. **Breaking-ish:** a remote Postgres
  that does not speak TLS, or presents an untrusted certificate, now fails to
  connect where it previously connected in cleartext; the error names both
  escape hatches. Applies only to builds with the off-by-default
  `croniq-store/postgres` feature.

  *`jwt.secret` is restricted on Windows.* The file was written with mode
  `0600` on Unix, but the non-Unix branch was a plain write, so on Windows it
  inherited the data directory's ACL — typically `Users:(RX)` under
  `C:\ProgramData`, readable by every local account. Since that one file both
  signs every token and derives the TOTP at-rest key, it is the file that least
  deserved a permissive ACL. It is now created empty, restricted to the current
  user's SID via `icacls /inheritance:r /grant:r`, and only then filled, so the
  secret never exists on disk under the inherited ACL. A failure to apply the
  ACL aborts startup rather than writing the key unprotected.

  *Shell-runner jobs no longer inherit the runner's environment.*
  `croniq-shell-runner` called `env_clear()` and then re-injected all of
  `std::env::vars()`, so every `runner shell {}` / `runner exec {}` job saw the
  runner process environment — including its own `CRONIQ_API_KEY`. Jobs now
  inherit an allowlist: `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `TMPDIR`,
  `TZ`, the `LANG`/`LC_*` locale set, and the Windows variables a subprocess
  genuinely cannot start without (`SYSTEMROOT`, `COMSPEC`, `PATHEXT`, `TEMP`,
  `TMP`, `APPDATA`, `PROGRAMFILES`, …). `CRONIQ_*` is never inherited
  implicitly. `CRONIQ_RUNNER_ENV_PASSTHROUGH` widens the list by name, or
  restores blanket inheritance with `*` — which still withholds `CRONIQ_*`,
  because a blunt wildcard must not hand out the runner's credentials.
  **Breaking-ish:** a job that silently relied on an inherited variable
  (`AWS_ACCESS_KEY_ID`, `JAVA_HOME`, a proxy setting) stops seeing it; declare
  it in the job's `env {}` block or add it to the passthrough list.

  *A failed privilege drop refuses to spawn.* In the same file, a `user`
  directive the runner could not honour — a non-numeric value on unix, or any
  value off unix — was logged and ignored, and the job then ran as the runner's
  own user, possibly root. That is strictly more privilege than the job asked
  for, so it is now a hard failure naming the two ways forward: set a numeric
  uid, or drop the directive and run the runner process itself as the desired
  user. Matches the .NET runner SDK's behaviour (#442).

  *The MCP mutation gate denies what it cannot classify.* `check_mutation_scope`
  only inspected bodies that parsed as an object with `method == "tools/call"`;
  a JSON-RPC batch (a top-level array, carrying no `method`) or an unparseable
  body fell straight through the `if let` and reached rmcp without ever meeting
  the `mcp:write` check. Not a confirmed bypass — rmcp 1.5 implements the MCP
  revision that removed batching — but a gate should refuse what it cannot
  read. Both now return `400`. JSON-RPC *responses* to server-initiated
  requests still pass, since they invoke nothing. Tool classification is
  deny-by-default too: croniq-mcp gained a `READ_TOOL_NAMES` list next to
  `MUTATION_TOOL_NAMES`, a `tool_requires_write()` helper that returns `None`
  for anything in neither, and a test asserting the two lists partition the
  router exactly — so a newly added tool fails CI rather than landing in the
  permissive half. The module doc comment, which named 5 mutation tools where
  the list has 17, was corrected.

  *Demo mode cannot be exposed to the network.* `CRONIQ_DEMO_MODE=1` seeds
  `admin`/`demo-admin`, a fixed admin-scoped API key, and (with
  `CRONIQ_DEMO_MFA=1`) the literal recovery code `123456` in all ten slots —
  everything needed to take an instance over is in this repository, so the only
  protection is unreachability. The server now refuses to start in demo mode if
  `--listen` or `--metrics` resolves to a non-loopback address (the default
  `:4000` means `0.0.0.0`), and `docker-compose.yml` publishes its ports to
  `127.0.0.1` instead of every interface. A container must bind `0.0.0.0` for a
  published port to reach it at all, so `docker-entrypoint.sh` sets
  `CRONIQ_DEMO_CONTAINER_BIND=1` and the server warns instead of refusing
  there; for the compose stack the guarantee is the host-side publish.

- **Access tokens no longer survive a password change, reset, or
  deactivation** (issue #431). Access tokens are stateless JWTs valid until
  `exp`, up to an hour. Refresh was correctly blocked after a deactivation — it
  re-checks `is_active` — but every access token minted beforehand kept working
  for the rest of its lifetime, so "I reset the password to lock the attacker
  out" did not actually lock them out for up to an hour. Each user row now
  carries a `token_generation` counter (migration `025`, `BIGINT`/`INTEGER`
  defaulting to `0`), stamped into every access token as a claim and compared
  against the row on each JWT-authenticated request. It is incremented on
  exactly three events — `POST /v1/users/me/change-password`,
  `POST /v1/auth/password-reset/confirm`, and `PATCH /v1/users/{id}` with
  `is_active: false` — and deliberately not on profile or role edits, since
  signing someone out is a real cost and a role change already propagates on
  the next refresh. A deleted user's tokens stop working immediately. The check
  costs one primary-key lookup per JWT-authenticated request, bringing the JWT
  path in line with the API-key and PAT paths, which already did store I/O per
  request; API keys and PATs themselves carry no claim, since both are already
  re-checked against their own revocation columns. Rolling restarts are safe:
  tokens minted by an older binary carry no claim and read as generation `0`,
  which is what every existing row is backfilled to, so the upgrade itself
  signs nobody out.

- **Permissive CORS replaced by an explicit allowlist, and security headers
  added to every response.** The API router applied `CorsLayer::permissive()`
  to every route — `Access-Control-Allow-Origin: *` with any method and any
  header — so any website could read every unauthenticated response
  (`/version`, `/v1/auth/config`, the password-reset and OIDC flows)
  cross-origin. Harmless today because authentication is Bearer-header only
  with no cookies anywhere, but a standing hazard the moment cookie auth
  appears. CORS is now derived from the operator-configured public app URL
  (`server { app_url … }` / `CRONIQ_APP_URL`): when set, exactly that origin
  is allowed, with the methods and headers the dashboard uses
  (`GET`/`POST`/`PUT`/`PATCH`/`DELETE`, `Authorization`, `Content-Type`) and
  never `Allow-Credentials`; when unset — the standard setup, where
  croniq-server serves the SPA itself and everything is same-origin — no CORS
  headers are emitted at all. Deployments that serve the dashboard from a
  different origin (a `VITE_API_URL` build) must set `app_url` for browser
  calls to keep working; non-browser clients are unaffected.

  Every response — API, dashboard, and `/mcp` — additionally carries
  `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`,
  `Referrer-Policy: no-referrer`, and a `Content-Security-Policy` verified
  against the Vite production bundle (`script-src 'self' 'wasm-unsafe-eval'`
  for the DSL wasm bridge, `style-src 'self' 'unsafe-inline'` for React style
  attributes, `connect-src 'self'`, `frame-ancestors 'none'`, `object-src
  'none'`, `base-uri 'self'`). The SPA holds the auth token in JS, so the CSP
  is the main thing limiting the blast radius of any future XSS.
  `Strict-Transport-Security` is deliberately not set — croniq-server does
  not terminate TLS; operators who do should add HSTS at their proxy. See the
  new *HTTP hardening* section in [`docs/operations.md`](docs/operations.md),
  which also documents the standing advice to keep the unauthenticated
  `/metrics` listener on an internal interface.
- **The login surface is hardened against enumeration, guessing, and
  lockout abuse.** Four measures, none of which changes a request or
  response schema apart from the new `429` (issue #428):

  *The second factor now has an attempt limit.* Password login has always
  had an account lockout, but the TOTP step had no equivalent: a caller
  holding a valid password plus an `mfa_token` could try 6-digit codes
  unthrottled for the token's full 5-minute TTL, against the ~3 codes that
  the ±1-step skew window keeps live at any moment. Five failed second
  factors now invalidate the `mfa_token` itself, forcing a fresh password
  login; the budget is per token rather than per account, so it cannot be
  used to lock a victim out of their own second factor. Malformed requests
  (neither code nor recovery code) and server faults do not count against
  it. `mfa_token`s additionally carry a unique `jti` claim so that two
  logins in the same second no longer produce the identical token —
  without it, invalidating one would have followed the user into the next
  attempt.

  *A verified TOTP code can no longer be replayed.* The highest consumed
  30-second time step is recorded per user, and a code from a step at or
  below it is rejected, so a code observed in transit is dead the moment
  it is used rather than staying valid for the rest of its window.
  Recovery codes were already single-use.

  *The username timing oracle is closed.* An unknown username returned
  `401` immediately, before any hashing, while an existing one paid for a
  bcrypt cost-12 verification — a difference an attacker can measure. The
  no-such-user branch now burns one bcrypt verification against a constant
  hash, mirroring the symmetry `password-reset/request` already had with
  its unconditional `202`. A locked account answers the same generic `401`
  as a wrong password too; it previously answered `403`, which confirmed
  the account exists, since only existing accounts can be locked.

  *`POST /v1/auth/login` and `/v1/auth/login/totp` are throttled per source
  address.* The per-account lockout is the right defence against online
  brute force, but on its own it is also a denial-of-service lever: anyone
  who guesses a username — `admin` being the obvious one — could keep that
  account locked with five bad logins every 15 minutes. A self-contained
  in-memory sliding window (30 attempts per 5 minutes, keyed by the socket
  peer address) now answers `429` beyond the budget, complementing rather
  than replacing the lockout. `X-Forwarded-For` is deliberately not
  parsed — it is attacker-controlled on a directly exposed server — so
  deployments behind a reverse proxy see the proxy's address here and
  should throttle at the proxy instead.

- **Password length rules are consistent across every entry point.** User
  create, change-password, password-reset confirm, and invitation accept
  required 8 characters while `croniq init` accepted anything non-empty,
  so the very first admin password could be weaker than any password set
  later. All five now share one constant pair in `croniq-auth`
  (`PASSWORD_MIN_LEN` / `PASSWORD_MAX_BYTES`) instead of four scattered
  literals. The new explicit upper bound of 72 bytes makes bcrypt's silent
  truncation visible: a longer password is refused with a clear message
  rather than quietly having its tail ignored. The demo Docker stack moves
  from `admin/admin` to `admin/demo-admin` to satisfy the same rule — the
  minimum is enforced with no demo-mode exception.

- **Issued credential scopes are bounded by the caller's own scopes.** The
  four endpoints that mint credentials — `POST /v1/users/me/tokens`,
  `POST /v1/api-clients`, `PUT /v1/api-clients/{id}` and `POST /v1/api-keys`,
  plus `POST /v1/api-clients/{id}/tokens` which hands out a client's scopes
  as a JWT — validated only that the caller was allowed to *use* the
  endpoint, not that the scopes being granted were covered by the credential
  presented on the request. A deliberately narrow token was therefore not a
  boundary: it could issue a wider one. All five now reject any scope the
  caller does not itself hold with a 403, via the shared
  `CallerContext::can_grant_scopes` check; `admin` is the wildcard and stays
  unrestricted. A PAT can no longer issue further PATs at all — chaining let
  a token outlive the revocation of the one it came from.
- **Runner identity is bound to the authenticated caller in the work protocol.**
  The work handlers (`POST /v1/work/poll`, `…/ack`, `…/renew`,
  `…/{execution_id}/events`) took the acting `runner_id` from the request body
  and verified only the caller's scope, never that the caller was the runner it
  named. Since `runner_id`s are operator-chosen names, a credential holding a
  `work:*` scope could interfere with another runner's work. Deployments where
  several runners hold their own credentials should upgrade.

  A `runner_id` is now bound to the credential that first uses it
  (first-writer-wins, persisted in the new `runner_identities` table) and every
  work request is checked against that binding. The events endpoint, which is
  addressed by execution, is fenced on the runner that claimed the execution,
  and the completion compare-and-swap now fences on the caller's identity rather
  than on the `runner_id` supplied in the body.
  `DELETE /v1/runners/{id}` releases a binding — the supported way to hand a
  `runner_id` to a different credential.

  Upgrading needs no preparation and does not break deployments that share one
  runner key across many runners: every such runner resolves to the same
  credential, so every check matches. Binding is inert without auth or without a
  store, and `pull_api { runner_identity_binding "off" }` restores the previous
  behaviour. No request or response schema changed; the only new observable is a
  `403` on those four endpoints (and a `503` if the binding lookup itself
  fails). See the README's *Runner identity in the work protocol* section and
  [`docs/operations.md`](docs/operations.md).
- **Redact secrets from the Live Console stream and restrict it to
  admins.** `GET /v1/events/stream` carries the raw server-wide tracing
  feed plus a replay buffer of recent events, but only required
  `executions:read` — a scope every role default hands out, including
  Viewer. It now requires the `admin` scope, matching what the endpoint
  actually exposes; per-execution output is unaffected and stays on
  `executions:read` via `GET /v1/executions/{id}/logs`. The console fan-out
  additionally drops events on secret-bearing tracing targets
  (`croniq::password_reset`, `croniq::email`, `croniq::oidc`) and replaces
  the value of any field named like a credential (`token`, `secret`,
  `password`, `confirm_url`, …) with `[redacted]`. Public identifiers such
  as `job_key` and `runner_id` are untouched.
- **Password-reset links no longer pass through `tracing`.** The
  reset-issued log line keeps its `user_id` / `reset_id` fields but drops
  the confirm URL, which embeds the single-use reset token. Operators
  without a mail transport still get the link to hand over: it is written
  straight to the server's stderr, bypassing the console hub, the event
  ring buffer, and OTLP log export. With a delivering mail transport the
  token is not printed at all. The dashboard hides the Console entry for
  non-admins and explains the 403 when the page is opened directly.
- **Reserved `__` metadata namespace is now enforced on every metadata
  ingress.** The `__`-prefixed metadata namespace belongs to the scheduler and
  the DSL compiler (`__runner_exec`, `__require`, `__prefer`,
  `__max_concurrent`) and runners act on those keys directly.
  `POST /v1/trigger` has always dropped caller-supplied keys in that
  namespace; the MCP `enqueue_job`, `job_trigger` and `create_job` tools and
  `POST /v1/jobs` did not, and forwarded them into the dispatch queue and the
  stored job definition unchanged. All four now apply the same filter, which
  moved into `croniq-config` as the single source of truth
  (`is_reserved_metadata_key`, `strip_reserved_metadata_map`,
  `strip_reserved_metadata_json`), so the namespace stays scheduler-owned no
  matter which API a caller reaches for. Callers keep influencing routing
  through the documented `require` / `prefer` fields; non-reserved metadata is
  unaffected.
- **Dependency advisories fixed by lockfile bumps.** The first cargo-deny run
  surfaced six fixable advisories, all resolved by semver-compatible bumps in
  `Cargo.lock`: `h2` 0.4.14 → 0.4.16 (unbounded empty DATA frames),
  `tokio-postgres` 0.7.17 → 0.7.18 (panic on a short `DataRow`),
  `postgres-protocol` 0.6.11 → 0.6.12 (unbounded SCRAM iteration count and a
  panic decoding malformed `hstore` values — both only exploitable by a
  malicious/untrusted database server), `anyhow` 1.0.102 → 1.0.104 (unsound
  `Error::downcast_mut`), `crossbeam-epoch` 0.9.18 → 0.9.20 (invalid pointer
  dereference in a `fmt::Pointer` impl), and the yanked `spin` 0.9.8 → 0.9.9.
  On the UI side, `npm audit fix` bumped the transitive `nanoid` pin past
  GHSA-2v37-7h3g-55p8 (infinite loop in custom generators with size 0), taking
  `npm audit --omit=dev` in `ui/` to zero findings; the two SDK npm trees were
  already clean.
- **Download verification fails closed.** `install.sh` previously continued
  with an unverified binary — warning only — when the release's `SHA256SUMS`
  could not be fetched or no sha256 tool was present. Both cases are now hard
  errors, with a new `--insecure-skip-verify` flag as the explicit escape
  hatch (e.g. for releases that predate the checksum file). The Dockerfile's
  `wasm-pack` fetch, previously an unverified `curl | tar xz` of a pinned
  version, now downloads to a file and verifies a pinned SHA256 (per build
  arch) before extracting.

- **.NET SDK: the shell-exec handler can be scoped to explicit job keys, and
  its privilege directives fail closed
  ([#442](https://github.com/nuetzliches/croniq/issues/442)).**
  `AddCroniqShellHandler()` registered the shell handler only as the
  catch-all default, so a runner that wanted shell-exec for one job had to
  grant it for every job key the server dispatches. A new
  `AddCroniqShellHandler("deploy:run", …)` overload registers the handler for
  the listed keys only and is now the documented preferred form; the
  parameterless catch-all stays as a deliberate opt-in equivalent to the Rust
  `croniq-shell-runner`. Two silently-unsafe payload fields now fail closed:
  the `user` directive — which .NET cannot honour (no setuid) and which was
  accepted and ignored, running the command as the runner's own user — fails
  the execution with a clear message, and payload-supplied `env` names that
  can hijack process resolution or library loading (`PATH`, `PATHEXT`,
  `COMSPEC`, `LD_PRELOAD`, `LD_LIBRARY_PATH`, `DYLD_*`, `CRONIQ_*`,
  case-insensitive) are rejected unless the new
  `CroniqShellHandlerOptions.AllowUnsafeEnvironment` opt-out is set.

- **SDK hygiene bundle: health-check host disclosure, auth on injected HTTP
  clients, SnakeYAML `SafeConstructor`, dependency scoping
  ([#443](https://github.com/nuetzliches/croniq/issues/443)).** Six
  individually-minor findings from a security review of `sdks/`, none urgent on
  its own, all cheap to close.

  *The .NET health check no longer echoes exception text.* `CroniqRunner`
  passed `ex.Message` from a failed poll into the runner state probe, and
  `CroniqRunnerHealthCheck` rendered it into the result description.
  `HttpRequestException` and `SocketException` messages routinely embed the
  resolved host and port ("No such host is known. (croniq.internal:4000)"), so
  an unauthenticated reader of `/health` learned the internal Croniq hostname.
  No credential was ever exposed — API keys never appear in these messages —
  and the stock ASP.NET Core response writer emits only the aggregate status,
  so this surfaced only behind a custom or dashboard response writer, which is
  common enough to be worth closing. A new `CroniqRunner.DescribePollFailure`
  now derives a fixed category from the exception *type* — `connection failed`,
  `http status <code>`, `poll timed out`, `poll failed` — and only that reaches
  the description; the full `ex.Message` is unchanged in the log line, which is
  operator-only. The probe's property was renamed `LastPollError` →
  `LastPollFailureReason` (both `internal`) so the invariant is readable at the
  type.

  *The Python SDK applies auth per request, so an injected HTTP client keeps
  the configured credential.* `CroniqClient` baked the `Authorization` header
  into the `httpx.AsyncClient` it constructed, which meant passing `http=` — the
  documented path for mTLS, proxies and custom transports — produced a client
  with no credential at all. Against a correct server that fails closed (401,
  then the retry loop), but if the injected client carried its own broader
  `Authorization`, `RunnerOptions.api_key` was silently ignored and the runner
  authenticated as somebody else. `TriggerClient` already did this correctly,
  with a comment explaining why; `CroniqClient` now follows it, applying
  `_auth_headers()` at each of the five call sites, which also means the
  configured credential overrides any header the injected client sets.

  *The Python quickstart reads its key from the environment.* `README.md` and
  the `croniq_runner` / `TriggerClient` docstrings showed
  `api_key="croniq_..."` inline while the Go and TypeScript samples used
  `os.Getenv` / `process.env`; they now use `os.environ["CRONIQ_API_KEY"]` and
  `os.environ["CRONIQ_TRIGGER_KEY"]`, so a copy-paste does not land a literal
  key in source control.

  *The Java conformance harness pins `SafeConstructor`.* `CaseLoader` and
  `YamlSupport` used a bare `new Yaml()`. Not exploitable as shipped —
  SnakeYAML 2.x's default `TagInspector` rejects global tags (the CVE-2022-1471
  fix) and the input is repo-local fixtures — but that safety is a
  version-dependent default, so both now construct
  `new Yaml(new SafeConstructor(new LoaderOptions()))`, which is
  version-independent.

  *The published Go SDK is stdlib-only.* `gopkg.in/yaml.v3` sat in
  `sdks/go/go.mod` although only the conformance harness imported it, so every
  consumer inherited a dependency no importable package could reach — dead
  weight in their module graph and live surface in their advisory scans. The
  harness moved to its own never-published module at `sdks/go/conformance/`
  (added to `go.work`, wired into the CI and release workflows, carrying a
  `replace` that `otel/go.mod` deliberately may not). `sdks/go/go.mod` now has
  an empty `require` block and an empty `go.sum`.

  *The .NET audit suppressions are scoped to the projects that need them.*
  `Directory.Build.props` applied four OpenTelemetry `NuGetAuditSuppress`
  entries to every project in the tree, including `Croniq.Runner.Sdk`, which
  references no OpenTelemetry package — a suppression on a project that cannot
  hit the advisory only degrades that project's audit signal, and would
  silently swallow the same GHSA arriving later through an unrelated
  dependency. The list stays in one place but is now conditioned on a
  `CroniqUsesOpenTelemetry` property that only the OTel SDK project and the
  demo app set.

  The issue's closing suggestion — negative conformance cases for hostile
  server responses — was already delivered as cases 13 and 14 with #441/#452.

### Fixed

- **Runner SDKs treat a `403` on the work endpoints as fatal
  ([#437](https://github.com/nuetzliches/croniq/issues/437)).** Since
  [#436](https://github.com/nuetzliches/croniq/issues/436) bound a runner's
  identity to the authenticated caller, `/v1/work/poll`, `…/ack`, `…/renew`
  and `…/{execution_id}/events` answer `403` when the credential does not own
  the `runner_id` the request names. All six SDKs retried that forever on the
  poll interval — five seconds by default — so a runner fenced out by an
  operator mistake looked *idle* rather than misconfigured: no work arrived,
  nothing crashed, and the only trace was a warning per poll (`debug` in
  Java, i.e. invisible). Unlike the `409` of a duplicate deployment, which
  can resolve itself when the other process exits, a `403` is permanent —
  retrying cannot clear it.

  Every SDK now stops on the first poll `403` with an error naming the
  `runner_id` and the two fixes: give the runner its own `runner_id`, or
  release the existing binding with `DELETE /v1/runners/{id}`. Rust and .NET
  reuse their existing conflict-streak machinery with an effective threshold
  of 1 (`ClientError::WorkOwnershipDenied` / `RunnerOwnershipDeniedException`)
  and leave the `409` counter untouched, since it reports something else. Go
  returns an `*OwnershipDeniedError` from `Runner.Run` after draining;
  Python raises `RunnerOwnershipDeniedError`; TypeScript rejects `run()` with
  `RunnerOwnershipDeniedError`; Java throws `CroniqOwnershipDeniedException`.
  The `409` path is deliberately untouched everywhere — Go's
  `TestRunnerSurvives409PollAndKeepsPolling` still pins retry-forever for it.

  Ack, renew and log-event failures no longer flatten the status either. A
  `403` on any of them is now logged at error level with the same remedy,
  because each has its own visible consequence: an unacked execution stays
  claimed until its lease expires, a refused renew means the lease expires
  mid-handler, and a refused event batch means the execution produces no log
  output at all. The Rust renew loop stopped discarding its result outright
  (`let _ = renew_client.renew(…)`). Renew's `404`/`409` — routine when a
  renew races the runner's own completion, see #438/#447 — stay at debug in
  all six.

  Java additionally needed the status code plumbed out of the wire layer:
  `CroniqClient.ensureSuccess` collapsed every non-2xx into an `IOException`
  whose only record of the status was the message text, so no caller could
  branch without parsing strings. It now throws a typed
  `CroniqHttpException` carrying `statusCode()`, `operation()` and `body()`,
  and the generic poll-failure log moved from `debug` to `warn` to match the
  other five.

  Conformance case `15-poll-403-ownership-fatal.yaml` pins the wire
  behaviour: the mock answers `403` on every poll and the case asserts
  exactly one poll over a two-second window, which a runner retrying on a
  200 ms delay could not satisfy. It runs in all five bindings — the Java
  binding's hardcoded `SCOPE` allowlist was extended (a case missing from it
  is silently skipped with a green suite; that trap is
  [#453](https://github.com/nuetzliches/croniq/issues/453)), and the Java
  binding now also burns the full case window when an expectation carries a
  `max_count`, matching the other four, so ceiling assertions actually hold
  there.

- **`POST /v1/work/renew` is a real per-execution lease
  ([#438](https://github.com/nuetzliches/croniq/issues/438)).** The handler
  accepted a `RenewRequest { runner_id, execution_id }` but never read
  `execution_id`: it only bumped the runner's registry-wide `last_poll_at`, so
  a per-runner heartbeat was wearing a per-execution lease API. Two things
  followed from that. `{"renewed": true}` came back for executions the caller
  did not hold, that had already finished, or that never existed — the
  response asserted a renewal of something the server had not looked at. And
  because the watchdog's stale-claim reaper exempted every claim a live runner
  had listed as inflight, one renew kept the reaper off *all* of that runner's
  claims: a runner genuinely wedged on some executions still looked healthy
  for them as long as its renew timer ticked for one other.

  Renew now verifies the named execution: it must exist (`404` otherwise), be
  `claimed`, and be held by the calling runner (`409` otherwise — a renew that
  races the runner's own completion lands here, which is harmless and never
  retryable). Only then is that one execution's lease refreshed. Leases are
  tracked per execution alongside the runner registry, stamped at dispatch and
  refreshed by both renew and any poll that lists the execution as inflight;
  the reaper's liveness exemption now reads that per-execution record and
  requires it to belong to the claim's own runner, so an execution a runner
  stopped renewing is reaped on schedule while its siblings keep running. The
  runner's liveness heartbeat is still refreshed on a successful renew, since
  a renew does prove the process is alive.

  No request or response schema changed and no runner configuration changes:
  the SDKs already renew once per in-flight execution, which is exactly what
  the endpoint now expects. A renew arriving after its execution finished
  changes from `200` to `409`; all SDKs discard the renew result, so this is
  invisible to handlers (SDK-side logging of it is tracked separately). Lease
  state is in memory and refills from the next poll after a server restart,
  well inside the reaper's grace window. `openapi.yaml` documents the new
  semantics and the `404` / `409` responses; see also *Orphaned claims* in
  [`docs/operations.md`](docs/operations.md).

- **A misconfigured `timezone` is no longer silently ignored
  ([#426](https://github.com/nuetzliches/croniq/issues/426)).** Three related
  silences, all of which changed *when* jobs fired while `validate` stayed
  green:

  *An invalid IANA name.* `timezone Europe/Berln` passed `validate`, survived
  `compile` into the runtime config verbatim, and the loader turned it into
  UTC via `.parse().ok().unwrap_or(chrono_tz::UTC)` — so a one-character typo
  moved every wall-clock fire of the job by the zone's offset, permanently and
  without a log line. Zone names are now resolved in one place
  (`croniq_config::timezone`) which has no UTC fallback: `validate` and
  `compile` report an error with a did-you-mean suggestion
  (`did you mean 'Europe/Berlin'?`), a Croniqfile with an unknown zone fails
  the server load and any reload, and a persisted API schedule carrying one
  loads **paused** with a `config_error` instead of firing in the wrong zone.
  `POST /v1/schedules`, `PUT /v1/schedules/{id}` and `POST /v1/jobs/register`
  reject it up front with `400`.

  *The job-level spelling.* `timezone Europe/Vienna` written as a bare
  directive inside `job { }` was discarded — it only worked in `defaults { }`
  or as a schedule option, even though `defaults { }` accepting the same bare
  keyword is exactly what makes the job body look like it should too. It now
  works, with precedence: schedule option > job-level directive >
  `defaults { }`.

  *Every other unknown job directive.* `validate` inspected only specific job
  directives, so any unrecognised key in a job body was a no-op without a
  diagnostic — the hole [#403](https://github.com/nuetzliches/croniq/issues/403)
  closed for the operator blocks, one level down. Job bodies are now checked
  against the known-key table like every other block, including the sub-block
  names and the `retry` / `dead_letter` bodies. `calendar`, `not_before` and
  `not_after` written in the body get a message naming the schedule line they
  belong on, rather than a did-you-mean.

  **This rejects configurations earlier versions accepted.** A Croniqfile with
  a misspelled zone, or a typo'd/misplaced directive in a job body, now fails
  `validate` and the server load instead of running with the value dropped. Run
  `croniq validate` before upgrading: everything it now reports was already
  being ignored at runtime.

- **`validate` warns when a wall-clock schedule has no timezone
  ([#427](https://github.com/nuetzliches/croniq/issues/427)).** `every day at
  03:00` with no zone anywhere runs at 03:00 **UTC**. That default is
  deliberate and unchanged — croniq never reads the host's `TZ`, so one
  Croniqfile fires at the same instant in dev, staging and prod — but nothing
  said so, and on a non-UTC host the job simply ran an hour or two off what the
  file read, with the offset moving at every DST switch. `validate` now emits a
  warning (not an error, and it does not block a boot) for `every day at …`,
  `every <weekday> at …`, `every <n>th of month at …`, and for a job-level
  `window`, whose gate is likewise evaluated in the job's zone. Interval
  schedules stay quiet — they are zone-independent — and so does `once at …`,
  whose value is parsed as UTC regardless of any declared zone, which makes
  "declare a timezone" the wrong advice there.

  The job detail view now shows the **effective** zone next to `next fire`,
  read off the live trigger rather than the config text, so it is filled in for
  a job that inherits it from `defaults { }` and reads `UTC` for one that
  declares nothing. `GET /v1/jobs/states` carries it as the new nullable
  `timezone` field.

- **.NET SDK: POSIX shell commands reach `sh` verbatim
  ([#442](https://github.com/nuetzliches/croniq/issues/442)).** The shell
  handler interpolated the command into `/bin/sh -c "…"`, escaping `"` but
  not `\`, so a command containing escaped quotes or ending in a backslash
  was corrupted by .NET's re-parse of the `Arguments` string and shell jobs
  failed in hard-to-diagnose ways. The command now travels as a single argv
  entry via `ProcessStartInfo.ArgumentList` — no escaping round-trip,
  matching the Rust shell runner's `sh -c <command>`. The Windows branch
  keeps the raw `cmd.exe /c <command>` pass-through on purpose (`cmd` parses
  the remainder of the line itself; argv-quoting would corrupt it) and is
  pinned by a test.
- **Dead-letter `job_key` filter is bound as a query parameter.** The SQLite
  backend's `list_dead_letters` assembled its `job_key` predicate by
  interpolating the filter value straight into the SQL string, so a value
  containing quote characters altered the shape of the query instead of being
  compared as a literal. `job_key` and `limit` now travel through `?N`
  placeholders, matching the `list_executions` path next to it and the
  Postgres backend, both of which were already parameterised. `limit` is also
  clamped to 1000, as the audit-log listing already did.

## [0.31.0] - 2026-07-27

### Added

- **`--version` on both binaries
  ([#407](https://github.com/nuetzliches/croniq/issues/407)).** Neither
  `croniq` nor `croniq-server` generated the flag, so answering "which
  version is this?" meant starting the server and querying the HTTP API —
  not an option for a pulled-but-not-running image, or when the server
  won't start. Both clap commands now carry `version`, which picks up
  `CARGO_PKG_VERSION` and therefore reports exactly what `GET /version`
  and the MCP handshake do.
- **`doctor` finding for unbounded run history
  ([#405](https://github.com/nuetzliches/croniq/issues/405)).** Retention
  is opt-in by design (an upgrade must never delete history), which means
  the default configuration grows `executions` and `execution_logs` for as
  long as the server lives, and nothing said so. The new
  `retention.unbounded_history` finding fires when neither
  `server { execution_retention }` nor any `keep_last` is configured. It is
  informational and never turns `doctor`'s exit code non-zero — keeping
  history forever is a legitimate choice; the point is that the decision is
  visible.
- **`doctor` finding for undecryptable TOTP secrets
  ([#408](https://github.com/nuetzliches/croniq/issues/408)).**
  `totp.secrets_undecryptable` (critical) counts confirmed TOTP secrets that
  no longer unwrap with the active JWT secret — a precise signal for the
  otherwise near-undiagnosable state described below.
- **Reload warns about boot-only settings
  ([#406](https://github.com/nuetzliches/croniq/issues/406)).** A reload
  re-reads the whole Croniqfile but only applies jobs, calendars, triggers
  and the `policy { }` flags; `server { }`, `pull_api { }` and the rest are
  boot-only and were dropped without a word. That is documented, but the
  reload still *looked* like a full apply — with `--watch` the operator
  edited the file, saw a successful reload, and had no indication that part
  of the edit hadn't landed (worst in container deployments, where a
  config-only change never restarts the process). Each changed boot-only
  setting is now named in a `WARN` with its old and new value, counted in
  the reload summary as `pending_restart`, and returned by
  `POST /v1/admin/reload-config` in a `pending_restart` array. The
  `alerts { }` block is compared by fingerprint rather than by value, so a
  channel's HMAC signing key can't reach the log.

### Changed

- **The server load path runs semantic validation and fails closed
  ([#402](https://github.com/nuetzliches/croniq/issues/402)).**
  `loader::load_file` ran parse → compile → load and never called
  `croniq_config::validate`, so every diagnostic the validator uniquely
  contributes was silently accepted at boot *and* by `croniq-server
  doctor` — which operators use as a pre-deploy gate. A duplicate job key
  dropped one of the jobs (`jobs=2 triggers=1`, exit 0), a job without a
  schedule was never scheduled, an unknown runner type compiled to nothing,
  and the `ephemeral` + `singleton` rejection from #302 applied only to
  `croniq validate`, not to the server that schedules the job. Boot, reload
  and `doctor` now reject these: the server refuses to start (listing every
  error at once), a reload is rejected with the running config left intact,
  and `doctor` reports one critical finding and exits `1`. Calendar rule
  failures and unresolvable `calendar` references stay per-job faults —
  the affected jobs load paused under `policy { strict_calendars }`
  (#361) — so one broken calendar still cannot take the scheduler down.
- **Unknown directives are config errors
  ([#403](https://github.com/nuetzliches/croniq/issues/403)).** Directives
  inside `server { }`, `pull_api { }`, `defaults { }`, `auth { }`,
  `observability { }`, `mcp { }`, `policy { }`, `smtp { }` and `oidc { }`
  (and their sub-block names) were dropped without a diagnostic by both the
  server and `croniq validate`, while a typo one nesting level up — an
  unknown top-level block or calendar rule type — was already a hard parse
  error. A silently ignored setting is hard to notice precisely because
  nothing misbehaves: a mistyped `execution_retention` left run history
  growing forever, and the only signal was a missing line in the boot log.
  Unknown keys now error with a did-you-mean suggestion, or the block's
  known key list when nothing is close.
- **`totp.enforced_without_enrollment` no longer claims a lockout
  ([#409](https://github.com/nuetzliches/croniq/issues/409)).** The finding
  said affected users "are refused at login" and recommended switching
  enforced 2FA **off** to fix it — a security downgrade prompted by an
  incorrect message, and one an operator follows under pressure. Login has
  handed those accounts an inline enrolment token since enforced 2FA
  shipped, so the boot log contained two contradictory warnings in the same
  startup. The finding now describes inline enrolment, drops the
  `required false` remedy (that belongs to the genuine lockout case,
  #408), and is `info` rather than `warning` — it is the expected state
  right after enforcement is switched on. The boot notice and the finding
  share one source of truth and a test asserts they agree.

### Fixed

- **A leftover `pull_api { auth … }` line is no longer silently ignored
  ([#408](https://github.com/nuetzliches/croniq/issues/408)).** #371
  established that the directive's value was used verbatim as the JWT
  signing secret, and 0.29.0 removed it — but that value is also the
  HKDF-SHA256 input for the AES-256-GCM key that wraps stored TOTP secrets
  at rest. Ignoring the line on upgrade therefore falls through to a
  freshly generated `$DATA_DIR/jwt.secret`, silently rotating the wrap key
  and making **every stored TOTP secret undecryptable** — with nothing in
  the boot log to say the secret source had changed. The line is now a
  config error whose message names the migration (move the value to
  `CRONIQ_JWT_SECRET`, then delete the line). On top of that, a failed
  unwrap during login logs a dedicated error naming the likely cause and
  the recovery path instead of discarding the `CryptoError`, and
  `docs/operations.md` documents the coupling and the rotation order.
- **The login page no longer reports every 5xx as "cannot reach server"
  ([#410](https://github.com/nuetzliches/croniq/issues/410)).** A transport
  failure and an HTTP error *response* are opposite situations — the latter
  proves the server was reached and failed internally — but both produced
  "Cannot reach server. Check that the Croniq backend is running.", sending
  the operator to check proxy, DNS and container status while
  `/v1/auth/config` and `/health` stayed green. The concrete case is the
  `500` above. The reachability wording is now reserved for a genuine
  transport error; a `500` reads "Server error during sign-in (HTTP 500) —
  the backend is reachable but failed internally, check the server logs",
  and 502/503/504 name the proxy/upstream layer. Classification moved into
  a unit-tested pure function, which also makes the existing "Invalid
  credentials." branch reachable again (`apiFetch` throws a bare
  `Unauthorized` error for 401, which the old message-prefix check missed).
- **Documented that retention does not shrink the database file
  ([#404](https://github.com/nuetzliches/croniq/issues/404)).**
  `execution_retention` and `keep_last` delete rows, but nothing runs
  `VACUUM` and no migration sets `PRAGMA auto_vacuum`, so on SQLite the
  freed pages are reused and the file stays at its high-water mark. Since
  enabling retention is usually a reaction to "the database got bigger than
  I expected", the reasonable conclusion was that retention wasn't working.
  `docs/operations.md` now states that pruning caps growth without
  returning space to the filesystem, what an explicit `VACUUM` costs (a
  full rewrite under an exclusive lock, roughly the DB size in free space),
  why `auto_vacuum` isn't a drop-in on an existing database, and the
  `VACUUM FULL` equivalent on PostgreSQL.
- **`openapi.yaml` was not valid YAML, and nothing checked it
  ([#412](https://github.com/nuetzliches/croniq/pull/412)).** Two failure
  modes, both from unquoted plain scalars carrying YAML-significant
  punctuation: a `description` containing `": "` made the whole document
  fail to load, and four `{ description: … }` flow mappings contained
  commas, which split the value at the first comma and invented
  null-valued keys — silently truncating
  `{ description: Unknown state, bad code, or invalid ID token }` to
  "Unknown state" while raising no parse error at all. Validating the
  repaired file against OpenAPI 3.1 surfaced 50 further pre-existing
  violations, also fixed: 38 response objects missing the required
  `description`, and 12 path parameters used in URL templates but never
  declared, leaving generators with no name or type to bind. Since the five
  language SDKs are generated from this file, a mis-parse here is a real
  breakage. A new CI job now parses the spec, rejects null-valued keys (the
  precise diagnostic for the comma case, which the schema error does not
  name), and runs full OpenAPI 3.1 validation, with the validator and
  PyYAML versions pinned so an upstream release cannot turn an unrelated PR
  red.
- **Unknown UI routes rendered a blank page
  ([#416](https://github.com/nuetzliches/croniq/pull/416)).** The route
  table had no catch-all, so a path matching nothing made `<Routes>` render
  `null` — a white page with no navigation and no way back, which reads as
  "the app is broken" rather than "wrong URL". A `path="*"` route now sits
  inside `Layout` (and therefore inside `ProtectedRoute`), so an
  authenticated operator keeps the sidebar and gets a link home while a
  logged-out one is bounced to `/login` like anywhere else. `NotFoundPage`
  is imported eagerly rather than lazily — the last-resort fallback should
  not depend on a chunk fetch that could itself fail.

### Security

- **Dependency bumps resolving 11 open Dependabot alerts, all HIGH
  ([#415](https://github.com/nuetzliches/croniq/pull/415),
  [#413](https://github.com/nuetzliches/croniq/pull/413)).**
  `react-router` 7.18 → **8.3.0** (RSC-mode CSRF bypass; the advisory has
  no 7.x backport, so the major bump *is* the fix — Croniq's UI uses the
  declarative API and no RSC, so real exposure was negligible),
  `postcss` → 8.5.18 (path traversal via source-map auto-loading),
  `js-yaml` → 4.3.0 (quadratic CPU consumption via merge-key chains) and
  `brace-expansion` (DoS via exponential expansion) across `ui/` and both
  TypeScript SDK directories, plus `quinn-proto` 0.11.14 → 0.11.16
  (transitive, `Cargo.lock` only). The react-router major was verified
  against the running dev stack before merging — client-side navigation,
  deep links, `useParams`, `useSearchParams` (read and write),
  `useMatch`/`NavLink`, history, and the `ProtectedRoute` redirect — since
  a type-check and a build cannot catch changed runtime routing behaviour.

## [0.30.0] - 2026-07-20

### Added

- **Watchdog recovery counters on `/metrics`
  ([#389](https://github.com/nuetzliches/croniq/issues/389)).** The
  watchdog's recovery actions were visible only as WARN logs and audit
  events, though their frequency is a key operator signal (frequent reaps =
  unstable runner; stranded cancels = jobs deleted with work in flight).
  `/metrics` now emits `croniq_watchdog_requeued_total{reason="dead_runner"
  |"stale_claim"|"reconciled"}`, `croniq_watchdog_cancelled_total{reason=
  "queue_ttl"|"stranded"}`, `croniq_watchdog_sla_missed_total`, and
  `croniq_watchdog_missed_fires_total`. See the new watchdog-metrics
  section in `docs/operations.md`.
- **Takeover audit trail + runner-identity-flapping detection
  ([#374](https://github.com/nuetzliches/croniq/issues/374) follow-up).**
  Every inline takeover (new `instance_id` under an existing `runner_id`)
  now records a `runner.takeover` audit event alongside the existing
  warning. On top of that the server detects takeover ping-pong: the #382
  fencing converges a duplicate deployment to a stable winner only when the
  fenced loser stays exited — under a container restart policy it restarts
  with a fresh `instance_id`, re-takes the identity, and the two processes
  evict each other forever (jobs keep running, so the churn is easy to
  miss). Three or more takeovers of one `runner_id` within 10 minutes now
  log a `runner identity flapping` warning and record a
  `runner.identity_flapping` audit event, throttled to once per window
  while the flapping lasts. Remedy stays the same: give each replica its
  own `runner_id`. See the expanded "Orphaned claims" section in
  `docs/operations.md`.

### Changed

- Removed the vestigial `dead_threshold_secs` parameter from the runner
  registry: `register_or_update_with_ttl` is merged into
  `register_or_update`, since instance takeover no longer depends on a
  dead-threshold (immediate takeover with deposed-instance fencing,
  [#374](https://github.com/nuetzliches/croniq/issues/374)). Internal API
  only; no behaviour change.

### Fixed

- **Calendar-gated jobs no longer wrongly exhaust across a closed gate
  ([#391](https://github.com/nuetzliches/croniq/issues/391)).** A calendar-
  or window-gated trigger now jumps straight to the next gate-open instant
  instead of stepping raw schedule ticks — the old 366-tick walk gave
  `every 1 minute` only a ~6h horizon, so any overnight/weekend gap
  exhausted the trigger and it showed as `overdue`. Trigger restore also
  heals `next_fire_at` values a pre-fix build persisted inside a closed
  gate, so a stale "overdue" never survives a restart.
- **API-registered schedules now get their calendar attached at runtime
  ([#393](https://github.com/nuetzliches/croniq/issues/393)).** Schedules
  created via `POST/PUT /v1/schedules`, `POST /v1/jobs/register`, `/adopt`,
  boot-time DB reconciliation, and hot-reload persisted and displayed a
  `calendar` name but never attached the compiled calendar to the runtime
  trigger, so the job ran **ungated** (fail-open, silently). Calendar names
  are now resolved against the union of DSL and store calendars (DSL wins
  on collision) and the gate attached; an unresolvable reference fails
  closed (trigger paused + `config_error`) under `strict_calendars`,
  matching the DSL path. `POST/PUT /v1/schedules` reject an unknown
  calendar name with 400; editing or deleting a calendar
  (`PUT/DELETE /v1/calendars/{id}`) now propagates to running triggers.
  Also fixes `DELETE /v1/schedules/{id}` never telling the scheduler to
  stop the job (a deleted schedule kept firing until restart).
- **Adopted non-interval jobs survive reload
  ([#395](https://github.com/nuetzliches/croniq/pull/395)).** `adopt` and
  the DSL synth path persisted a job's human-readable schedule *summary*
  (`every 5 minutes`) into `cron_expression`, which nothing could parse
  back — so adopted `daily`/`weekly`/`monthly`/`once` jobs silently
  vanished from the scheduler on the next reload/restart, and the live
  push on adopt sent nothing. Schedules are now persisted as canonical,
  re-parseable DSL and rebuilt for every schedule shape. Hardened the
  lexer against an unbounded token push (OOM) on an unexpected character,
  since the schedule parser now runs on stored data.

## [0.29.0] - 2026-07-18

### Fixed

- **Singleton jobs no longer deadlock on claims orphaned by a fast runner
  restart ([#374](https://github.com/nuetzliches/croniq/issues/374)).** A
  `claimed` execution whose runner process died without going *Dead* in the
  registry (fast redeploy re-registering the same `runner_id` within the
  lease TTL, or a server restart wiping the in-memory registry) was never
  reclaimed by any sweep. For a `singleton` / `max_concurrent` job that one
  stale row saturated the concurrency slot forever: every new fire piled up
  to `max_queue_depth` and the job logged `skipping execution — queue
  overflow` indefinitely. Two changes close this:
  - **Stale-claim reaper** in the watchdog sweep: `claimed` executions whose
    claim age exceeds the job `timeout` (default `5m`) plus a grace window
    (`max(2 × lease_ttl_secs, 120 s)`, i.e. 4 minutes on defaults) are
    flipped back to `queued` (same attempt — an orphaned claim is an infra
    fault and must not burn retry budget or dead-letter a healthy job) and
    re-enqueued, independent of runner liveness. Claims a live runner still
    reports inflight are exempt, so a slow-but-alive handler is never
    double-run. Each reap logs a warning and records an
    `execution.stale_claim_requeued` audit event. Runs after the SLA sweep,
    so a stuck claim still fires its `job_sla_missed` alert before recovery.
  - **Immediate takeover on runner restart**: a new `instance_id` polling
    under an existing `runner_id` now takes the identity over (and the old
    session's claims are requeued) without waiting for the old entry to
    cross the dead threshold — a fresh instance id from the same persisted
    runner id *is* the restart signal. The duplicate-deployment protection
    from #190 is preserved by fencing: the deposed instance id keeps getting
    `409 Conflict` (→ the SDKs' #134 conflict-streak bail-out), so two live
    processes sharing a `runner_id` converge to a last-writer-wins winner
    instead of endlessly evicting each other. Operators running rolling
    deploys with overlap should prefer stop-before-start or distinct
    `runner_id`s — the draining old instance's in-flight claims are requeued
    at takeover and may run twice.

  The previously documented workaround (stop the runner for longer than
  `lease_ttl` so the dead-runner sweep fires) is obsolete; wedged jobs now
  self-heal within one watchdog sweep after `timeout + grace`.

- **Late completions can no longer overwrite a requeued execution
  ([#374](https://github.com/nuetzliches/croniq/issues/374) follow-up).**
  `complete_execution` / `complete_as_dead` had no state guard: a completion
  arriving after the watchdog requeued an orphaned claim (dead-runner sweep,
  stale-claim reaper, or restart takeover) flipped the requeued — possibly
  already re-claimed — row to `completed`/`failed`/`dead` anyway, clobbering
  the re-run's result, dead-lettering a job another runner was healthily
  re-running, and falsely freeing a singleton slot. Both store methods are
  now compare-and-swap (only rows still `claimed`, and only for the runner
  that reported the completion); a late completion is acknowledged but
  ignored with a `late completion ignored` warning — no retry, no dead
  letter, no failure alert. The poll dispatch path additionally drops work
  items whose store claim is refused instead of handing a runner an
  execution it can never legally complete.

- **Store-queued executions can no longer strand outside the work queue
  (#385).** A requeue that flipped a row to `queued` but couldn't rebuild its
  work item (job config unresolvable at that moment) left the row invisible
  forever — nothing re-reads store-queued rows after boot. A new
  queued-reconcile watchdog sweep re-enqueues such rows, and cancels rows
  whose job exists neither in the DSL nor as a stored definition (audit event
  `execution.stranded_queued_cancelled`). `WorkQueue::enqueue` is now
  idempotent per execution id, so concurrent producers can't double-dispatch.

- **Watchdog sweeps list the oldest claims first (#384).** The SLA sweep and
  the stale-claim reaper read claimed executions through a bounded query that
  previously returned newest-first — with more than 500 concurrent claims the
  oldest rows (exactly the hung/orphaned ones both sweeps target) fell
  permanently outside the window. A dedicated `list_claimed_older_than` query
  now returns oldest claim first, so overflow only defers the newest claims
  to a later sweep.

- **Runner status endpoints honor the configured `pull_api.lease_ttl`
  (#383).** `GET /v1/runners`, the runners SSE stream, `/health`, `/metrics`,
  and the MCP status tools derived Online/Stale/Dead from a hardcoded 120 s
  dead-threshold, so with a non-default `lease_ttl` the displayed status
  diverged from the watchdog's actual liveness assessment (e.g. UI "dead"
  while the watchdog still treated the runner as alive). All of them now use
  the configured TTL; the hardcoded helpers are deprecated.

### Removed

- **`pull_api { auth … }` directive** (#371). The directive was never an
  "auth on/off" switch: its value was consumed verbatim as the JWT signing
  secret, so `auth none` merely set the secret to the literal string `none`
  while scope enforcement on `/v1/work/*` stayed on — a runner without a
  seeded key still got `401`. Worse, it only applied at server boot: the CLI
  (`croniq init`, TOTP at-rest encryption) never saw it, so setting it silently
  diverged CLI-side encryption from server-side decryption. The JWT secret is
  now resolved solely from `CRONIQ_JWT_SECRET` or an auto-generated
  `$DATA_DIR/jwt.secret`, shared with the CLI via
  `croniq_auth::jwt_secret::ensure`, so the two always agree. The directive is
  dropped from `Croniqfile.demo` / `Croniqfile.example` and the config
  generator; a lingering `auth …` line in an existing `pull_api` block is now
  silently ignored. Never put a secret in the Croniqfile.

## [0.28.0] - 2026-07-18

### Added

- **Dead-letter policy for API-registered jobs** (closing the documented v1 gap
  from the stale-replay guard). `job_definitions` gains
  `dead_letter_retention`, `dead_letter_operator_hint`, and
  `dead_letter_replay_max_age` (migration 023, all NULL = system default —
  matching the migration-004 pattern for `dead_letter_enabled`), so jobs
  created via `POST /v1/jobs`, `POST /v1/jobs/register`, or the MCP
  `create_job`/`update_job` tools carry the same `dead_letter { … }` policy a
  Croniqfile job declares. The replay endpoint's stale-replay guard
  (`409 stale_replay` unless `force: true`) now applies to API jobs with a
  configured `replay_max_age`; previously it only ever fired for DSL jobs. The
  UI's New/Edit Job dialogs expose the three fields under the dead-letter
  toggle, and the job detail page shows the retention and replay-guard values.

- **Executions carry their original logical fire time (`scheduled_for`)**. Jobs
  whose logic is coupled to their scheduled time (e.g. a monthly report deriving
  the period from "the fire moment − 1 month") previously had no reliable signal:
  `fire_at` was reset to `now + backoff` on every retry and to `now` on
  dead-letter replay, so a run that landed late computed against the wrong
  instant. A new `scheduled_for` timestamp is stamped at the trigger's logical
  fire time and carried unchanged through the entire retry chain and across
  dead-letter replay (`fire_at` keeps its "when this row becomes due" meaning).
  It is persisted (migration 022, backfilled from `fire_at`), returned on the
  `Execution` and `DeadLetter` API objects, and delivered to runners on the
  work-poll assignment (`scheduled_for`, `null` when the server predates the
  field — runners must not fall back to `fire_at`). Manual triggers set it to
  the trigger moment.
- **Stale-replay guard for dead letters (`dead_letter { replay_max_age … }`)**.
  Opt-in: when set, replaying a dead letter whose original `scheduled_for` is
  older than the given duration is refused with `409 stale_replay` (a structured
  body carrying `scheduled_for`, `age_seconds`, and `replay_max_age`) unless the
  request passes `force: true`. Guards against re-running a time-coupled job
  (e.g. a monthly invoice) against the wrong period long after it dead-lettered.
  Applies to `POST /v1/dead-letters/{id}/replay` and the MCP `dlq_retry` tool
  (which gains a `force` flag). No policy set → replay is always allowed (the
  UI still surfaces the age). The in-browser DSL generator emits the field.
- The Dead Letters page now shows each letter's **original scheduled time**, and
  a stale replay prompts a confirm dialog ("originally scheduled X ago — replay
  anyway?") that retries with `force`.
- **All six runner SDKs expose `scheduled_for` on the handler context** (Rust,
  TypeScript `scheduledFor`, Python `scheduled_for`, Go `ScheduledFor`, Java
  `scheduledFor()`, .NET `ScheduledFor`). Handlers can now read the trigger's
  logical fire time — stable across retries and replay — instead of wall-clock
  now, which is what makes a time-coupled job (e.g. a monthly report) correct
  after a late or replayed run. Absent (older server) surfaces as
  `null`/`None`/zero, never a silent fall back to the queue fire time.

### Changed

- **A job whose `calendar` gate does not resolve now fails closed** (issue #361).
  Previously, if a referenced calendar failed to compile — or was not defined —
  the loader dropped it with a `WARN` and the job fired **un-gated**, on exactly
  the days it was configured to skip; both startup and hot-reload reported
  healthy. Such a job is now loaded **paused** with a surfaced reason (an `ERROR`
  log, a `config_error` field on `GET /v1/jobs/states`, a
  `croniq_config_calendar_faults` metric, and a distinct badge in the UI), so it
  cannot fire without its gate. Fixing the calendar and reloading re-arms the job
  automatically. **This changes behavior on upgrade** for deployments that
  currently boot with a broken-but-referenced calendar. To restore the old
  warn-and-skip behavior, set `policy { strict_calendars false }` in the
  Croniqfile — this escape hatch is temporary and will be removed in a future
  release.
- **Dead-letter replay now reuses the job's configured timeout** instead of a
  hard-coded `5m`, and falls back to the job's `runner { require/prefer }` when
  the dead letter's metadata doesn't carry them. Replay also emits a
  `dead_letter.replayed` audit event.

### Fixed

- **`weekday`/`weekend` aliases now work in weekly calendar rules**
  ([#356](https://github.com/nuetzliches/croniq/issues/356)). `croniq fmt` and
  the cron→DSL converter emit the group aliases and `croniq validate` accepted
  them, but the scheduler's calendar compiler did not — a formatted Croniqfile
  failed to load its calendar, and the jobs bound to it lost their gate. The
  parser, the scheduler compiler, and validation now share one argument parser
  (`croniq_config::calendar_args`), so they cannot diverge again;
  `croniq validate` catches bad window/monthly/annual arguments offline, and
  `POST`/`PUT /v1/calendars` rejects rules the loader can't compile. Cosmetic:
  API payloads of DSL-defined calendars show the expanded day names instead of
  the literal alias.
- **Dead-letter replay is atomic.** The replay execution insert and the
  dead-letter removal now happen in a single store transaction. Previously a
  failure between the two separate writes left a `queued` execution that was
  never handed to a runner (it only ran after a restart catch-up) while the
  dead letter stayed replayable — replaying it again duplicated the run. Two
  concurrent replays of the same dead letter can no longer both succeed (the
  loser gets a `404`).
- **A failed execution-row persist no longer enqueues ghost work.** The retry
  path (and `POST /v1/trigger`) enqueued the work item even when writing the
  execution row failed, handing a runner an `execution_id` with no backing
  row — status updates and the final completion targeted a nonexistent
  execution and the run vanished without history. A retry whose row cannot be
  persisted now terminates the chain into a replayable dead letter (when
  dead-lettering is enabled) so it stays operator-visible; a trigger rejects
  with `500` so the caller can retry.
- **`dead_letter { retention 0 }` now actually keeps dead letters forever.**
  The pipeline stamped `expires_at = now`, so the very next purge sweep deleted
  the row — the opposite of the documented "no TTL" semantics. Zero retention
  now persists a `NULL expires_at`, which the sweep deliberately skips.

## [0.27.0] - 2026-07-17

### Added

- **Turn dead-lettering off from the Croniqfile**
  ([#348](https://github.com/nuetzliches/croniq/issues/348)). The execution
  engine already honored a disabled dead-letter policy end-to-end, but the DSL
  had no way to express it — `dead_letter { }` only understood `retention` and
  `operator_hint`, so dead-lettering was effectively always on (30d).
  `dead_letter { enabled false }` now drops an execution that exhausts its
  retries instead of queuing it for triage; usable per job or as a global
  `defaults { dead_letter { enabled false } }` ("off by default, opt in for the
  jobs that actually get triaged"). The in-browser DSL generator emits the
  block, and the job create/edit dialogs gain a "dead-lettering enabled" toggle.
- **Bulk-delete for dead letters**. New `POST /v1/dead-letters/bulk-delete`
  (`dead-letters:write`) removes many at once — an explicit `ids` list, or
  `all: true` (optionally scoped to a `job_key`) to clear the queue — returning
  the number deleted. The Dead Letters page gains a "Clear all" action.

### Changed

- **`defaults { }` `retry` / `dead_letter` blocks now field-merge instead of
  replacing** ([#348](https://github.com/nuetzliches/croniq/issues/348)).
  Previously a job that declared its own `retry` or `dead_letter` block was
  re-parsed from the built-in defaults, silently discarding the `defaults { }`
  values (e.g. a job setting only `operator_hint` reverted `retention` to 30d).
  A job block now overrides only the fields it names and inherits the rest from
  `defaults { }` — consistent with how scalar directives (`timeout`, `timezone`)
  already inherit; a `retry` block with no strategy qualifier keeps the
  inherited strategy. Review any config that relied on the old
  reset-to-default behavior.

## [0.26.0] - 2026-07-16

### Added

- **Execution retention**
  ([#344](https://github.com/nuetzliches/croniq/issues/344)). Terminal
  executions were persisted forever, so run history grew unbounded. Two opt-in
  knobs now cap it, enforced by the existing 30 s watchdog sweep on both the
  SQLite and Postgres backends: a global `server { execution_retention <dur> }`
  age sweep that prunes `completed` / `failed` / `cancelled` executions (and
  their logs) older than the given duration (`30d`, `7d`, `12h`, …), and a
  per-job `keep_last N` cap (settable in `defaults { }` or a `job { }` block)
  that keeps only the newest N terminal executions of a job. `dead` executions
  are excluded from both — their lifecycle stays governed by dead-letter
  retention. Both are disabled by default (history is kept forever) so an
  upgrade never silently deletes run history; deletions run in bounded batches
  to keep SQLite write-lock time short, and `ephemeral` jobs are unaffected
  (their executions are never persisted). New partial indexes on
  `executions(completed_at)` back the prune (migration 021).

## [0.25.0] - 2026-07-16

### Added

- **Global maintenance switch**
  ([#342](https://github.com/nuetzliches/croniq/pull/342)). A manual toggle
  plus an optional scheduled `[start, end)` window freezes job dispatch
  server-wide: running executions finish, scheduled fires are skipped (the
  schedule still advances, so there is no catch-up burst when the window ends),
  and queued work plus triggers accepted during the window resume once it
  clears. Backed by a `maintenance` store singleton (migration 020,
  SQLite/Postgres), gated in the scheduler tick and the runner work-poll, and
  exposed via `GET` (any authenticated caller) / `PUT` (admin only)
  `/v1/maintenance`. The UI adds an admin-only topbar popover and an app-wide
  banner shown to every user while maintenance is active.
- **Entity cross-links across the UI**
  ([#342](https://github.com/nuetzliches/croniq/pull/342)). Reusable
  job/runner/execution links wire the dashboard, jobs, executions, runners,
  dead-letters, alerts, and the command palette together, plus a runner-scoped
  executions filter driven from the URL.
- **The running Croniq version is now shown in the app topbar**, and the topbar
  live indicator reflects real runner-stream (SSE) connectivity. The
  command-palette shortcut badge is platform-aware (`Ctrl K` on Windows/Linux,
  `⌘K` on macOS).

### Fixed

- **Dashboard "runners online" KPI counted zero**
  ([#342](https://github.com/nuetzliches/croniq/pull/342)). The runners SSE
  stream serialized runner status via `Debug` (`"Online"`) instead of serde
  (`"online"`), so once the shared stream fed the app-wide runners cache the
  case-sensitive KPI filter matched nothing. The stream now emits the same
  lowercase status as the REST `/v1/runners` endpoint.

## [0.24.1] - 2026-07-15

### Fixed

- **`croniq fmt` no longer drops the `ephemeral` / `queued` schedule
  modifier** ([#336](https://github.com/nuetzliches/croniq/issues/336)).
  The execution-mode prefix is the semantic source for a job's
  `execution_mode` and takes precedence over the `execution_mode`
  directive and a `defaults` block, so a dropped prefix silently flipped
  an ephemeral job to queued on `fmt -w` — also resetting `catch_up`
  (`none` → `all`) and `max_queue_depth` (`1` → `null`). The formatter
  now emits the prefix, so a round-trip through `fmt` is
  semantics-preserving (`compile` output is unchanged). As part of the
  fix the grammatical singular is kept for an interval count of 1
  (`every 1 minute`, not `every 1 minutes`); that rule is now shared
  across the `fmt`, schedule-summary, and cron-`convert` emitters so they
  can no longer drift.
- **`croniq quickstart` scaffolds the grammatical `every 1 minute`** in
  its template
  ([#339](https://github.com/nuetzliches/croniq/issues/339)), matching
  the formatter output above.

## [0.24.0] - 2026-07-15

### Added

- **`croniq-server` now runs on PostgreSQL at runtime**
  ([#326](https://github.com/nuetzliches/croniq/issues/326)), building on the
  CI-guarded store backend from
  [#298](https://github.com/nuetzliches/croniq/issues/298). Select it with
  `server { db postgres://… }` or the `CRONIQ_DB` env var (env wins). The
  synchronous Postgres driver runs on a dedicated OS thread behind a
  `PgStoreHandle` actor so it never blocks the async runtime, and an
  end-to-end CI job boots the server against a Postgres service container.
  SQLite remains the default.
- **`POST /v1/trigger` now sends a `Retry-After` header on the per-job
  queue-overflow `429`**
  ([#299](https://github.com/nuetzliches/croniq/issues/299),
  [#312](https://github.com/nuetzliches/croniq/issues/312)). Producers — and
  the SDKs, which surface it as `retryAfterMs` — get an explicit backpressure
  hint (seconds) telling them how long to wait before retrying, instead of
  hammering a full queue. Previously the `429` carried no such hint.
- **The DSL generator on the docs site now emits complete, paste-ready
  Croniqfile blocks.** `/generator.html` was extended from bare
  schedule/calendar fragments to full `job` / `calendar` blocks, the
  top-level config blocks (`server`, `pull_api`, `mcp`, `oidc`,
  `observability`, `defaults`, `alerts`), and job options (`runner
  shell`/`exec`, retry, `singleton`/`max_concurrent`, and more)
  ([#324](https://github.com/nuetzliches/croniq/issues/324) and follow-ups).

### Fixed

- **`openapi.yaml` re-synced with the running server.** The spec version now
  tracks the release, and it documents live routes that were missing
  (`GET /v1/tags`, `GET /v1/jobs/states`, `GET /v1/system/diagnostics`,
  `GET /v1/auth/config`, and the `GET /v1/alerts/deliveries` pair) plus the
  `429` backpressure response on `POST /v1/trigger`.

## [0.23.0] - 2026-07-14

### Added

- **Rust SDK: first-class trigger (producer) client
  ([#286](https://github.com/nuetzliches/croniq/issues/286)).** The Rust runner
  SDK (`croniq-runner-sdk`) gains `TriggerClient`
  (`TriggerClient::builder(url).api_key(...).build()`) wrapping
  `POST /v1/trigger`, at parity with the .NET producer client
  ([#277](https://github.com/nuetzliches/croniq/issues/277)):
  `client.trigger(job_key).metadata(...).require(...).prefer(...).timeout(...)
  .idempotency_key(...).send().await` → `TriggerResult { execution_id, queued,
  deduplicated }`. It carries its **own** credentials (`.api_key` /
  `.bearer_token`) — triggering needs the `jobs:trigger`/`admin` scope, distinct
  from the runner's poll key — omits unset optionals (`metadata`, `require`,
  `prefer`, `timeout`, `idempotency_key`) from the request body, forwards
  arbitrary JSON `metadata` verbatim, and surfaces the server's `deduplicated`
  flag (defaulting to `false` for older servers,
  [#279](https://github.com/nuetzliches/croniq/issues/279)). Non-2xx responses
  surface as `TriggerError` — the per-job queue-overflow `429`
  ([#299](https://github.com/nuetzliches/croniq/issues/299)) as a dedicated
  `TriggerError::QueueOverflow` variant, other statuses as
  `TriggerError::Server { status, body }`. A Rust conformance runner is wired to
  the shared producer cases in `sdks/conformance/cases-trigger/`
  ([#287](https://github.com/nuetzliches/croniq/issues/287)) and runs them
  automatically once those cases are present.
- **Java SDK: first-class trigger (producer) client
  ([#285](https://github.com/nuetzliches/croniq/issues/285)).** `CroniqTriggerClient`
  wraps `POST /v1/trigger` for the Java SDK at parity with the .NET client
  ([#277](https://github.com/nuetzliches/croniq/issues/277)). `trigger(...)` takes a
  `job_key` plus optional `metadata`, `require`/`prefer`, `timeout`, and
  `idempotency_key`, and returns `TriggerResult { executionId, queued, deduplicated }`.
  The client carries its own credentials (`CroniqClientOptions` — the
  `jobs:trigger`/`admin` scope, deliberately separate from the runner's poll keys),
  omits unset optional fields from the wire body, defaults `deduplicated` to `false`
  on older servers that omit it, and surfaces non-2xx responses as
  `CroniqTriggerException` — including the per-job queue-overflow `429`
  ([#299](https://github.com/nuetzliches/croniq/issues/299)), distinguished via
  `isQueueOverflow()`. Wired into the Java SDK's conformance runner against the shared
  trigger (producer) cases ([#287](https://github.com/nuetzliches/croniq/issues/287)).

- **Go SDK: first-class trigger (producer) client
  ([#282](https://github.com/nuetzliches/croniq/issues/282)).** The Go SDK
  gains `croniq.TriggerClient` (`NewTriggerClient(...)`) wrapping
  `POST /v1/trigger`, at parity with the .NET producer client
  ([#277](https://github.com/nuetzliches/croniq/issues/277)):
  `Trigger(ctx, *TriggerRequest)` → `TriggerResponse{ExecutionID, Queued,
  Deduplicated}`. It carries its **own** credentials (`WithAPIKey` /
  `WithBearer`) — triggering needs the `jobs:trigger`/`admin` scope, distinct
  from the runner's poll key — omits unset optionals (`metadata`, `require`,
  `prefer`, `timeout`, `idempotency_key`) from the request body, forwards
  arbitrary JSON `metadata` verbatim, surfaces the server's `deduplicated`
  flag (defaulting to `false` for older servers,
  [#279](https://github.com/nuetzliches/croniq/issues/279)), and returns
  non-2xx responses — including the per-job queue-overflow `429`
  ([#299](https://github.com/nuetzliches/croniq/issues/299)) — as a
  `*croniq.ServerError` callers can key off `.Status`. The Go conformance
  binding is wired to the shared producer cases in
  `sdks/conformance/cases-trigger/`
  ([#287](https://github.com/nuetzliches/croniq/issues/287)) and runs them
  automatically once those cases are present.
- **First-class trigger (producer) client for the TypeScript SDK
  ([#284](https://github.com/nuetzliches/croniq/issues/284)).** Brings the
  Node SDK to parity with the .NET producer client
  ([#277](https://github.com/nuetzliches/croniq/issues/277)):
  `createTriggerClient(...)` / `CroniqTriggerClient.trigger(jobKey, { metadata,
  require, prefer, timeout, idempotencyKey })` wraps `POST /v1/trigger` and
  returns `{ executionId, queued, deduplicated }`. It is independent of the
  runner and carries its own `jobs:trigger`-scoped credentials (runner poll
  keys typically lack that scope). Unset optionals are omitted from the JSON
  body; `idempotency_key` drives server-side dedup
  ([#279](https://github.com/nuetzliches/croniq/issues/279)) with a missing
  `deduplicated` flag parsed as `false`; the per-job queue-overflow `429`
  ([#299](https://github.com/nuetzliches/croniq/issues/299)) surfaces as a
  `QueueOverflowError` (subclass of `HttpError`, carrying `retryAfterMs`) so a
  batching producer can back off. The TypeScript conformance binding gains a
  trigger runner that attaches to the shared producer cases
  ([#287](https://github.com/nuetzliches/croniq/issues/287)) as they land.
- **Wire-level trigger (producer) cases in the shared conformance suite
  ([#287](https://github.com/nuetzliches/croniq/issues/287)).** The suite
  previously modelled only the runner (consumer) loop. A new
  `sdks/conformance/cases-trigger/` directory — with its own
  `schema/trigger-case-schema.json` — pins the `POST /v1/trigger` producer
  contract so every SDK's trigger client (#282–#286) validates against one
  language-neutral spec instead of hand-rolled per-SDK tests. Cases cover the
  minimal request (`job_key` only), the full request with snake_case
  serialisation and omission of unset optionals (`metadata`, `require`,
  `prefer`, `timeout`, `idempotency_key`), `ApiKey` auth with the producer's
  own credentials, idempotency dedup (`deduplicated: true` surfaced; a missing
  flag parsed as `false`) and oversized-key rejection, the `TriggerResponse`
  shape (`execution_id`, `queued`, `deduplicated`), non-2xx errors, and the
  per-job queue-overflow `429` from
  [#299](https://github.com/nuetzliches/croniq/issues/299). Producer cases
  live in a separate directory from runner cases so existing consumer-only
  bindings keep passing untouched. CI validates the new cases against the new
  schema across every SDK pipeline.
- **First-class trigger (producer) client for the Python SDK
  ([#283](https://github.com/nuetzliches/croniq/issues/283)).** `croniq_runner`
  now ships `TriggerClient` / `TriggerClientOptions` / `TriggerResult`, wrapping
  `POST /v1/trigger` at parity with the .NET producer client
  ([#277](https://github.com/nuetzliches/croniq/issues/277)):
  `await client.trigger(job_key, metadata=…, require=…, prefer=…, timeout=…,
  idempotency_key=…)`. The client carries its own `jobs:trigger`-scoped
  credentials (independent of the runner), omits unset optionals from the body
  (never `null`), forwards `idempotency_key` for server-side dedup
  ([#279](https://github.com/nuetzliches/croniq/issues/279)) — surfacing
  `deduplicated` and defaulting it to `false` on older servers — and raises on
  non-2xx, including the per-job queue-overflow `429`
  ([#299](https://github.com/nuetzliches/croniq/issues/299)). The Python
  conformance binding is wired up to run the shared trigger cases from
  [#287](https://github.com/nuetzliches/croniq/issues/287).

### Fixed

- **Postgres store backend now compiles and is verified in CI
  ([#298](https://github.com/nuetzliches/croniq/issues/298)).** `PgStore`
  implemented only 5 of the 12 store traits, so
  `cargo build -p croniq-store --features postgres` failed to compile — and
  because no CI job built with `--features postgres`, the regression went
  unnoticed (the backend is advertised in `AGENTS.md` but couldn't be built or
  used). The seven missing traits (`AuthStore`, `JobDefinitionStore`,
  `TriggerDefinitionStore`, `CalendarDefinitionStore`, `DslAdoptionStore`,
  `ExecutionLogStore`, `AlertStore`) are now implemented — mirroring the SQLite
  backend's semantics — together with the matching Postgres schema migrations.
  A new CI job builds the feature, runs clippy on it, and executes an
  integration test against a Postgres service container so the backend can't
  rot again.

## [0.22.3] - 2026-07-14

### Fixed

- **Scheduler `tick` span no longer floods OTLP trace backends
  ([#310](https://github.com/nuetzliches/croniq/issues/310)).** With the `otlp`
  feature and `OTEL_EXPORTER_OTLP_ENDPOINT` set, the scheduler emitted one
  `tick` root span **every second at `INFO`**, unconditionally — pure
  scheduler-heartbeat noise that swamped any persistent trace backend
  (Tempo, Elasticsearch/APM, …) and skewed latency dashboards whether or not a
  job fired. The span is now emitted at `trace`, so it is off under the default
  `info` filter, consistent with the log-side denoise already in place (per-fire
  logs at debug/trace, a throttled `INFO` heartbeat for liveness —
  [#275](https://github.com/nuetzliches/croniq/issues/275)). Operators who want
  the per-tick span opt back in via `RUST_LOG`. Additionally, the OTLP **span**
  layer now carries the same `info`-default level filter (overridable via
  `OTEL_LOG_LEVEL`) that the OTLP log bridge already had: raising `RUST_LOG` for
  local debugging no longer ships every debug/trace span to the collector, so
  OTLP trace volume is decoupled from `RUST_LOG` and future hot-path spans can't
  regress the flood.
- **`POST /v1/trigger` now enforces the per-job queue-overflow cap
  ([#299](https://github.com/nuetzliches/croniq/issues/299)).** The scheduler
  bounds scheduled fires at `max_queue_depth` (per-job override, default 10), but
  the trigger endpoint enqueued directly and skipped the guard — a burst of
  triggers (event storms, client retries, a hot producer) could pile queued
  executions up unbounded for a single job. The endpoint now rejects with
  `429 Too Many Requests` once the per-job queued count is at the cap, checked
  after idempotency dedup (a coalesced trigger enqueues nothing) and before the
  execution row is persisted, so a rejected trigger leaves no orphan row behind.

## [0.22.2] - 2026-07-06

### Fixed

- **`singleton` / `max_concurrent` on an `ephemeral` job is now rejected instead
  of silently no-op'ing ([#302](https://github.com/nuetzliches/croniq/issues/302)).**
  The concurrency guard ([#278](https://github.com/nuetzliches/croniq/issues/278))
  counts persisted in-flight executions in the store, but `ephemeral` jobs
  deliberately do not persist their executions — so `singleton` / `max_concurrent`
  compiled clean yet provided zero overlap protection, a silent footgun for a
  fire-and-forget poll with external side effects. Config validation now errors
  on the combination (whether `ephemeral` comes from a schedule prefix, an
  `execution_mode` directive, or a `defaults {}` block), and the compiler no
  longer stamps the inert `__max_concurrent` metadata onto ephemeral jobs. Use
  `queued` (which persists executions) when a fire-and-forget poll must never
  overlap itself.

## [0.22.1] - 2026-07-06

### Changed

- **`POST /v1/trigger` ignores caller metadata in the reserved `__` namespace.**
  Internal keys (`__runner_exec`, `__require`, `__prefer`, `__max_concurrent`,
  …) are stamped by the scheduler / DSL compiler and consumed directly by
  runners; the trigger endpoint now drops any `__`-prefixed keys from the
  caller's `metadata` instead of merging them over the DSL-compiled values.
  Use the request's `require` / `prefer` fields to influence routing. DSL
  metadata and non-reserved caller keys are unaffected.

## [0.22.0] - 2026-07-06

### Added

- **Per-job concurrency guard: `singleton` / `max_concurrent N`
  ([#278](https://github.com/nuetzliches/croniq/issues/278)).** A runner pool
  could previously run two executions of the SAME job at once — a scheduled
  fire overlapping a still-running previous fire, or an on-demand
  `POST /v1/trigger`. Jobs can now declare `singleton` (shorthand for
  `max_concurrent 1`) or `max_concurrent N` in the Croniqfile; the compiler
  stamps the limit into the job's internal `__max_concurrent` metadata and the
  server enforces it at claim time: an execution of a guarded job is only
  handed to a runner while fewer than N executions of that job are claimed
  (counted from the store, the authoritative in-flight record). Blocked items
  stay queued at their FIFO position — skipped in place, so they neither get
  dropped nor starve other jobs' items behind them — and dispatch as soon as a
  slot frees. Declaring both directives, `max_concurrent 0`, or a non-numeric
  value is a config-validation error.
- **Idempotency keys on `POST /v1/trigger`
  ([#279](https://github.com/nuetzliches/croniq/issues/279)).** Event-driven
  producers operate under at-least-once semantics (event redelivery, client
  retries, concurrent publishers) and could enqueue duplicate executions
  for the same logical event. The trigger request now accepts an optional
  `idempotency_key` (max 200 chars, scoped per `job_key`): a repeat trigger
  carrying the same key coalesces to the existing execution — while that
  execution is still queued/running, or for a configurable window after it
  was created (`pull_api { trigger_dedup_window 10m }`, default 10 minutes)
  — and the response gains a `deduplicated` flag. Dedup is best-effort for
  at-least-once producers, not a strict exactly-once guarantee. Migration
  019 adds the nullable `executions.idempotency_key` column plus a partial
  `(job_key, idempotency_key)` index.
- **UI surfacing for both features
  ([#292](https://github.com/nuetzliches/croniq/pull/292)).** The execution
  detail shows the caller-supplied idempotency key (with copy button) when
  present; the manual Trigger button now reports the queued execution id and
  flags when the server coalesced onto an existing execution; the job
  overview's Routing card gains a Concurrency row (`singleton` pill /
  `max N in flight` / `unbounded`).
- **Ephemeral job dispatches now surface in the scheduler heartbeat
  ([#276](https://github.com/nuetzliches/croniq/pull/276),
  [#275](https://github.com/nuetzliches/croniq/issues/275)).** Ephemeral
  jobs only logged their dispatch at `DEBUG`, so at the default `INFO`
  level they left no server-side trace — an ephemeral job that stopped
  firing looked identical to a healthy one. The periodic scheduler
  heartbeat (`INFO`, ~5 min) now folds in per-job ephemeral dispatch counts
  since the last heartbeat as `ephemeral=[<key>:N, …]`, giving an
  observable liveness signal without per-fire log spam.

## [0.21.0] - 2026-06-22

### Added

- **`execution_mode` on the job scheduling-state API + ephemeral UI cues
  ([#266](https://github.com/nuetzliches/croniq/pull/266),
  [#263](https://github.com/nuetzliches/croniq/issues/263)).**
  `GET /v1/jobs/states` now reports each job's `execution_mode`
  (`queued` / `ephemeral`). The Jobs dashboard shows an **"ephemeral"** badge
  in the list and labels the execution mode in the job detail, and an
  ephemeral job's empty execution history is now explained as expected —
  rather than reading as a stalled or broken job.

### Fixed

- **Ephemeral jobs no longer wedge after a runner restart
  ([#266](https://github.com/nuetzliches/croniq/pull/266), fixes
  [#263](https://github.com/nuetzliches/croniq/issues/263)).** Ephemeral-mode
  jobs stopped firing and sat `overdue` forever once a runner restarted, and
  every ephemeral completion logged `execution not found for completion`. The
  scheduler now tracks dispatched ephemeral execution ids in a self-pruning
  set so a completion is acknowledged as a no-op instead of mis-reported as
  `NotFound`; ephemeral jobs also bypass the queue-depth/quota backpressure
  guards and use replace-latest enqueue (at most one queued item per job), so
  non-persisted work can no longer pile up past the cap and freeze the
  trigger.
- **Quota guard no longer wedges drained queued jobs
  ([#268](https://github.com/nuetzliches/croniq/pull/268)).** The per-job
  quota guard incremented an in-flight counter on every fire but never
  decremented it (no `release()` call path existed), so any persisted job
  stopped firing after `max_parallel` (default 10) fires and sat `overdue`.
  The unconfigurable parallel cap — redundant with the `max_queue_depth`
  overflow guard, which bounds in-flight work from live queue state — was
  removed; the self-healing per-minute trigger rate limit remains.

### Security

- **Dependency bumps resolving open Dependabot alerts
  ([#258](https://github.com/nuetzliches/croniq/pull/258),
  [#265](https://github.com/nuetzliches/croniq/pull/265)).** `vite`
  8.0.14 → 8.0.16 (fixes the `server.fs.deny` Windows alternate-path bypass
  and the launch-editor UNC-path NTLMv2 disclosure), `react-router` → 7.18.0
  (CSRF via PUT/PATCH/DELETE document requests), `js-yaml` → 4.2.0, and
  `@babel/core` → 7.29.7 across the UI and TypeScript SDKs.

## [0.20.1] - 2026-06-15

### Fixed

- **Topbar & control contrast polish
  ([#256](https://github.com/nuetzliches/croniq/pull/256)).** Three UI defects
  spotted in the running app: the `/alerts` and `/console` breadcrumbs
  rendered with a stray leading slash (unmapped routes echoed the raw
  pathname); the ⌘K search-shortcut badge (and search icon) could be squished
  on a narrow topbar; and secondary / ghost buttons plus status badges
  (StatusPill, `<Badge>`, tab counters) had no resting border, washing into
  light backgrounds until hovered. Unmapped routes now degrade to a clean
  Title-Case crumb, the shortcut badge keeps its size, and every button/badge
  keeps a defined edge at rest.

## [0.20.0] - 2026-06-12

### Added

- **`job_missed_fire` liveness alert + per-job fire metrics
  ([#253](https://github.com/nuetzliches/croniq/pull/253)).** A new alert
  trigger fires when a scheduled fire never happens — a job's persisted
  `next_fire_at` goes overdue past the rule's `expected_within` grace while
  the trigger is still active — catching a silently-stalled scheduler that a
  100%-success dashboard would otherwise hide. The watchdog sweep dedups per
  `(rule, job_key, next_fire_at)`. `/metrics` also gains
  `croniq_job_last_fire_timestamp`, `croniq_job_next_fire_timestamp`, and
  `croniq_job_overdue{job_key}` so external monitoring can alert on staleness
  even when the in-process scheduler is wedged and no run failed. The
  dashboard surfaces this too ([#254](https://github.com/nuetzliches/croniq/pull/254)):
  a new `GET /v1/jobs/states` endpoint backs an **"overdue" badge** on the
  Jobs list and a red "overdue" in the job's Next-fire KPI, so a stalled
  scheduler no longer reads as healthy behind `NEXT FIRE: —`.
- **Scheduler supervision + liveness signal
  ([#252](https://github.com/nuetzliches/croniq/pull/252)).** The scheduler
  task is now supervised: if it ever panics or exits, the process logs the
  cause and exits non-zero so the container's `restart:` policy recovers a
  fresh scheduler instead of running on with a silently-dead one. A new
  heartbeat is exposed on `/metrics` as
  `croniq_scheduler_last_tick_timestamp` (gauge) and
  `croniq_scheduler_ticks_total` (counter) so external monitoring can alert
  on a wedged scheduler even while HTTP keeps serving, plus a low-rate INFO
  "scheduler heartbeat — alive" log. Each tick is bounded by a 60 s timeout
  so a hung store/lock is logged and skipped (leaving the liveness metric
  stale) rather than wedging the loop forever.

### Fixed

- **Recurring clock-time schedules survive DST spring-forward
  ([#251](https://github.com/nuetzliches/croniq/pull/251)).** A daily /
  weekday / monthly job whose fire time fell in the spring-forward gap
  (e.g. `every day at 02:30` in `Europe/Berlin` on the last Sunday of
  March) used to exhaust its trigger permanently and silently —
  `next_fire_after` returned `None` for the non-existent wall-clock time.
  The scheduler now rolls a gap time forward to the transition instant and
  picks the earliest occurrence for fall-back ambiguity. An `Exhausted`
  trigger is also now treated as terminal only for non-recurring (`once` /
  `disabled`) schedules: a recurring schedule is re-armed on restart and on
  hot-reload instead of staying dead.

## [0.19.0] - 2026-06-04

### Added

- **SMTP email alert channel
  ([#230](https://github.com/nuetzliches/croniq/pull/239)).** Alerts can
  now be delivered over email. A new `smtp { }` DSL block configures the
  non-secret transport settings (host, port, from, TLS mode), while the
  credentials stay ENV-only via decomposed `CRONIQ_SMTP_USERNAME` /
  `CRONIQ_SMTP_PASSWORD` — they are never read from or written to the
  Croniqfile.
- **Operational alert rule overrides
  ([#231](https://github.com/nuetzliches/croniq/pull/240)).** Operators
  can snooze, disable, or throttle an individual alert rule at runtime
  without editing the DSL. Overrides carry a required operator note and
  an optional expiry; a snooze is just a time-boxed disable. The eval
  path merges overrides over the DSL config (a throttle override
  *replaces* the rule's DSL window rather than taking the min), the
  watchdog auto-clears expired overrides, and stale entries are pruned on
  boot. Writing overrides requires the admin-only `alerts:write` scope.
  Surfaced via `POST /v1/alerts/rules/{name}/{snooze,disable,throttle}`
  and `DELETE /v1/alerts/rules/{name}/override`; `GET /v1/alerts/config`
  now returns active overrides alongside the resolved config.
- **Alert rule overrides in the UI
  ([#242](https://github.com/nuetzliches/croniq/pull/242)).** Admins can
  drive the new override endpoints directly from the Alerts page: each
  rule gains snooze / disable / throttle controls and an active override
  renders inline as a pill with the operator's note. Non-admins see the
  override state read-only.
- **Deep-link from a job's Executions tab to the global Executions view
  ([#242](https://github.com/nuetzliches/croniq/pull/242)).** The per-job
  tab now links to `/executions` pre-filtered by `job_key`. Filters on
  the Executions page are URL-driven (`?state=&job_key=`), so the view is
  shareable and deep-linkable.

### Changed

- **Renamed "Run(s)" to "Execution(s)" across the UI
  ([#242](https://github.com/nuetzliches/croniq/pull/242)).** The data
  model has always called these *executions*; the UI now matches
  everywhere, both in user-facing labels (tabs, empty states, copy) and
  in internal identifiers (`RunBars` → `ExecutionBars`, `RunOutcome` →
  `ExecutionOutcome`, `.run-bars` → `.execution-bars`). Consistency was
  favored over shorter names.

### Fixed

- **OpenAPI timestamp drift + dead-letter job context
  ([#237](https://github.com/nuetzliches/croniq/pull/237)).**

## [0.18.0] - 2026-05-28

### Added

- **Restored CRUD surfaces lost in the operator-console redesign
  ([#228](https://github.com/nuetzliches/croniq/pull/228)).** The #143
  overhaul ported the visual layer but dropped several mutation paths
  that the API still exposed. This release re-wires them against the
  existing hooks; no backend changes.
  - **Jobs page** — the `+` button now opens a *New Job* dialog instead of
    being a no-op; Adopt / Unadopt buttons in the detail header take
    ownership of DSL-managed jobs (mirrors the existing CalendarsPage
    flow); the Schedule tab gains Add / Edit / Delete with a dialog that
    round-trips cron expression, timezone, calendar, window, and the
    enabled flag.
  - **Runners page** — converted to the established `.split` master/detail
    layout with `/runners/:runnerId` deep links. The detail pane shows
    identity, capacity ring, capabilities, tags, the 50 most-recent
    executions filtered by runner_id, and a Remove action — restoring
    the per-runner activity view that disappeared in the redesign.
  - **Executions page** — also converted to `.split` with `/executions/:id`
    routing. The detail pane mounts the existing `ExecutionDetail` +
    `LogsPanel` components, which were written but had no host route.
    Per-execution logs are reachable from the UI again.

### Changed

- **JobRow percentage chip now reports success-rate consistently.** It
  previously mixed failure-rate (red) with success-rate (green) on the
  same column, so a job with one failed run in 14 read as `7%` which
  scanned as a score in the low single digits. The chip now follows the
  same thresholds as the per-job detail header and the dashboard tile
  (100% green, ≥ 90% neutral, < 90% red).
- **Dashboard "Success rate (24h)" tile** picked up the same threshold
  coloring so the dashboard no longer reads identical at 99% and 70%.
- **Topbar breadcrumb** learned `/executions/:id` and `/runners/:id`
  segments (the chrome used to dump a 36-char hex blob in place of the
  page title); long current segments now ellipsize instead of pushing
  the search box off-screen.

### Fixed

- **Long master lists no longer expand the page.** `.split > .master`
  defaulted to `min-height: auto` (the grid-item default), which let the
  flex column grow past its track. Pinning it at `0` makes
  `.master-list { flex: 1; overflow-y: auto }` actually constrain the
  row list. Latent on `/jobs` and `/dead-letters` (short demo data),
  observable on `/executions` as soon as the list exceeded the
  viewport.
- **Removed an unused right-side `Sheet` drawer primitive
  (`ui/components/ui/sheet.tsx`).** The established design template for
  list+detail is the `.split` layout; the Sheet was leftover scaffolding
  that no page rendered.

## [0.17.3] - 2026-05-28

### Changed

- **Official Docker image now ships with the `smtp` cargo feature.** Until
  v0.17.2 the published `ghcr.io/nuetzliches/croniq` image was built with
  `--features croniq-server/otlp` only, so `CRONIQ_SMTP_URL` /
  `CRONIQ_SMTP_FROM` were silently ignored and the NoopSender stayed active
  even when configured — operators had to roll their own image to send
  invitation / password-reset emails. The release Dockerfile now builds with
  `--features croniq-server/otlp,croniq-server/smtp`, so the lettre-backed
  sender is available out of the box. Runtime behaviour is unchanged when
  `CRONIQ_SMTP_URL` is unset: the NoopSender stays active and the API keeps
  returning the token URL in its JSON response, identical to the off-build.
  No code, wire-protocol, or SDK API changes vs v0.17.2.

## [0.17.2] - 2026-05-28

### Fixed

- **Segmented 2FA input now accepts password-manager autofill
  ([#222](https://github.com/nuetzliches/croniq/pull/222)).** Bitwarden /
  1Password / browser autofill drop the full 6-digit code into the first
  box; the input previously truncated it (`maxLength={1}`) and only kept
  the last keystroke, so users saw a single digit in box 1. The first
  box now accepts the full code and multi-digit input is distributed
  across the boxes — same behaviour as paste.

## [0.17.1] - 2026-05-28

### Added

- **Config diagnostics — boot warnings, `doctor`, and endpoint
  ([#215](https://github.com/nuetzliches/croniq/pull/215)).** The server now
  surfaces missing / risky configuration at startup as actionable warnings,
  exposes the same report via `croniq-server doctor` (exits non-zero on
  critical findings, never binds a port), and over an authenticated
  endpoint for operators.
- **Inline TOTP onboarding + segmented 2FA code input
  ([#214](https://github.com/nuetzliches/croniq/pull/214)).** New users can
  enrol TOTP from the login flow itself instead of needing a separate
  Settings detour; the 6-digit input is now a segmented field with paste
  splitting and auto-advance.
- **Boot-time reconciliation of `CRONIQ_INIT_API_KEY`
  ([#217](https://github.com/nuetzliches/croniq/issues/217)).** On every
  start, the server now compares `CRONIQ_INIT_API_KEY` against the stored
  key for client `default` and logs the outcome (match, differs, or no
  default client). Previously the variable was silently ignored once the
  data dir existed, which made orchestrator-driven key rotation fail with
  an unexplained 401. To actually rotate the key from the env var (revoke
  existing active keys, install the new one), additionally set
  `CRONIQ_INIT_API_KEY_RECONCILE=1` — the default is log-only so an
  accidental env change cannot silently revoke a working credential.

### Fixed

- **DSL: `every` now accepts the compact duration form
  ([#216](https://github.com/nuetzliches/croniq/issues/216)).** `every 1m`,
  `every 30s`, `every 2h` are now parsed identically to the verbose
  `every N <unit>` form. Previously they were rejected with
  `expected number, got '1m'`, even though the same compact form was already
  accepted on `timeout` — the asymmetry made the error confusing.
- **Request-derived link base URL + Croniqfile `app_url`
  ([#212](https://github.com/nuetzliches/croniq/pull/212)).** Invitation,
  password-reset, and OIDC login links now derive the base URL from the
  request (`X-Forwarded-Proto`/`X-Forwarded-Host` / `Host`) when neither
  `CRONIQ_APP_URL` nor `server { app_url "…" }` is set, so links work
  behind a reverse proxy with no extra config. The new DSL setting takes
  precedence over the env var.

## [0.17.0] - 2026-05-27

### Added

- **Per-job metrics on the `/metrics` endpoint.** Three new series are
  computed from the executions store on each scrape — no schema change and no
  separately-persisted counters: `croniq_job_executions_total{job_key,state}`
  (terminal-state counter), `croniq_job_duration_seconds{job_key}` (a
  histogram with `_bucket`/`_sum`/`_count`), and
  `croniq_job_last_run_timestamp{job_key}` (gauge). Backed by a new
  `ExecutionStore::job_execution_metrics` aggregate query (one grouped scan).
  The previously planned `croniq_job_log_bytes_total` series remains a
  follow-up.
- **Enforced 2FA + single-request TOTP login
  ([#202](https://github.com/nuetzliches/croniq/pull/202)).** Opt-in
  `auth { totp { required true } }` / `CRONIQ_REQUIRE_TOTP` rejects logins from
  accounts without a confirmed authenticator (surfaced on
  `GET /v1/auth/config`). `POST /v1/auth/login` now accepts an inline `code` /
  `recovery_code` for a single-request login, and `GET /v1/users/me` returns
  `totp_enabled`.

### Fixed

- **Rust runner SDK now aborts the handler on a server-issued cancel.**
  The Rust SDK already polled at capacity so `PollResponse.cancel` arrived
  (issue #176 PR2), but it only logged a warning and let the handler run to
  completion. It now aborts the in-flight handler future for each cancelled
  execution and acks it as `failure` — matching the .NET / Go / Python /
  TypeScript SDKs and conformance cases 04 / 04a.
- **Settings 2FA disable flow
  ([#202](https://github.com/nuetzliches/croniq/pull/202)).** Disabling TOTP
  from Settings sent a 6-digit code where the server expected the account
  password, so it always failed with 401; the UI now sends the password and
  reflects the enabled/disabled state via a new `totp_enabled` field.
- **Invitation / password-reset / OIDC links now honour `CRONIQ_APP_URL`
  ([#205](https://github.com/nuetzliches/croniq/pull/205)).** They previously
  always pointed at `http://localhost:4000`.
- **Revoked personal access tokens are hidden from the token list
  ([#207](https://github.com/nuetzliches/croniq/pull/207)).** A revoked PAT
  looked identical to a live one in Settings; auth already rejects revoked PATs
  and the audit log keeps the revocation record.
- **Live Console filtering + light-mode readability
  ([#203](https://github.com/nuetzliches/croniq/pull/203)).** Level chips now
  filter client-side (instant toggle, no stream teardown) and the event tail
  stays a dark terminal in light mode.

### Security

- **`croniq init` / `quickstart` no longer leak secrets to a non-interactive
  stdout.** Both commands printed the seeded API key and the generated admin
  password straight to stdout, so piping the output into a log (Docker /
  systemd journal, CI runs, `tee init.log`) left those credentials in
  plaintext on persistent storage. They are now revealed inline only when
  stdout is a terminal; when stdout is redirected they are written to
  `$DATA_DIR/initial-credentials` (mode 0600 on Unix) and only the path is
  printed — the shape `kubeadm init` uses for `admin.conf`. A new
  `--print-secrets` flag forces the inline reveal for scripted setups that
  intentionally capture stdout. Closes the CodeQL `rust/cleartext-logging`
  sharp edge that was tracked in the roadmap.

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
