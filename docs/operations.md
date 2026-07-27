# Operations

Operational notes for Croniq deployments — environment variables, public
endpoints, and conventions you'll want to know when running a non-toy
instance.

## Public endpoints

These endpoints are reachable without authentication. They are safe to expose
to pre-login UI and to external uptime probes; they return no secrets.

| Endpoint | Purpose |
|---|---|
| `GET /health` | Liveness + queue/runner counters. Used by load balancers and the dashboard. |
| `GET /version` | Build and environment metadata (see below). |

### `GET /version`

```
GET /version  →  200
{
  "version":    "0.23.0",                 // from CARGO_PKG_VERSION
  "git_sha":    "f3aea44",                // short SHA, "unknown" outside a checkout
  "build_time": "2026-05-23T12:35:00Z",   // RFC 3339 UTC, baked in at build
  "env":        "production"              // from CRONIQ_ENV, defaults to "unknown"
}
```

The UI uses this to render the version chip on the login page and a colored
environment badge in the topbar (so an operator doesn't confuse staging with
production at a glance). Operators can also curl it during deploys to
confirm the rollout has actually replaced the previous build.

`version`, `git_sha`, and `build_time` are stamped at compile time by
[`crates/croniq-server/build.rs`](../crates/croniq-server/build.rs); only
`env` is read at request time, so changing `CRONIQ_ENV` does not require a
rebuild — just a restart.

## Environment variables

A complete table of supported variables lives in
[the project README](../README.md#environment-variables). The ones below are
specific to operational concerns and may be worth calling out separately.

### `CRONIQ_ENV`

Free-form label identifying the deployment environment. Surfaced by
`GET /version` as the `env` field and rendered as a badge in the UI when the
value is anything other than `production`.

| Value | Default | Notes |
|---|---|---|
| `CRONIQ_ENV` | `unknown` | Conventional values: `production`, `staging`, `dev`, `preview`. The UI treats `production` as the implicit default and shows no badge for it. |

The label is non-sensitive (no hostnames, no internal IPs) and is exposed
publicly via `/version`. Don't put credentials, infrastructure tags, or
anything you wouldn't print on a sticker into this variable.

### SMTP (`CRONIQ_SMTP_*` + `smtp {}` block)

The alert `email` channel, invitation mails, and password-reset mails all
route through a single SMTP transport, assembled at boot from two layers:

1. The Croniqfile `smtp {}` block — non-secret connection settings only
   (`host`, `port`, `security`, `from`).
2. The `CRONIQ_SMTP_*` environment — fills any field the block omits, and is
   the **only** place credentials may live (the Croniqfile is a read-only
   mount, so it must never carry a password).

The binary must be built with `--features smtp`. Without that feature the
transport degrades to a `NoopSender` that logs the recipient but sends
nothing, and the API returns the raw invite/reset URL in its response so an
admin can deliver it out-of-band. Boot diagnostics flag the mismatch as
`email.smtp_feature_missing` (critical) when `CRONIQ_SMTP_*` is set but the
feature is absent.

| Variable | Default | Notes |
|---|---|---|
| `CRONIQ_SMTP_URL` | — | Legacy composite URL (`smtp://user:pass@host:587/?tls=required`). When set it **wins** over everything below. |
| `CRONIQ_SMTP_HOST` | — | Relay host. Overridden by `smtp { host }`. Required (via either layer) for the decomposed path to activate. |
| `CRONIQ_SMTP_PORT` | `587` | Overridden by `smtp { port }`. |
| `CRONIQ_SMTP_SECURITY` | `starttls` | `starttls` \| `tls` \| `none`. Overridden by `smtp { security }`. |
| `CRONIQ_SMTP_FROM` | — | From address. Required for any real send. Overridden by `smtp { from }`. |
| `CRONIQ_SMTP_USERNAME` | — | Auth username. **Env-only.** |
| `CRONIQ_SMTP_PASSWORD` | — | Auth password. **Env-only.** |

**Precedence**, highest first: `CRONIQ_SMTP_URL` → per-field `smtp {}`
directive → matching `CRONIQ_SMTP_<FIELD>` env var → built-in default.

**`<VAR>_FILE` convention** — `CRONIQ_SMTP_URL`, `CRONIQ_SMTP_HOST`,
`CRONIQ_SMTP_USERNAME`, `CRONIQ_SMTP_PASSWORD`, `CRONIQ_INIT_API_KEY`, and
`CRONIQ_ADMIN_PASSWORD` each accept a `…_FILE` sibling that points at a file
whose trimmed contents supply the value. This is the recommended way to feed
Docker/Kubernetes mounted secrets without exposing them in the process
environment. If both the direct var and its `_FILE` are set, the direct var
wins.

## Authentication

Croniq supports two UI sign-in methods: username + password (the default),
and OIDC/SSO. Each can be turned off independently — operators on SSO-only
installs typically disable password login so it isn't an exposed attack
surface; the symmetric flip (SSO off, passwords on) is also supported by
simply not configuring an OIDC provider.

### `auth.password.enabled`

```hcl
auth {
  password {
    enabled false   # default: true
  }
}
```

When `false`, the public password-flow endpoints all return
`403 password_login_disabled` with the standard error envelope:

| Endpoint | Behaviour |
|---|---|
| `POST /v1/auth/login` | `403 {"error":"password_login_disabled", …}` |
| `POST /v1/auth/login/totp` | same (TOTP is step 2 of the password flow) |
| `POST /v1/auth/password-reset/request` | same |
| `POST /v1/auth/password-reset/confirm` | same |

API-key auth (`POST /v1/api-clients/{id}/tokens`) and personal access
token minting (`POST /v1/users/me/tokens`) keep working — those don't
involve the password endpoints. Existing JWTs and refresh tokens are
unaffected; the gate only blocks the path that issues new ones from a
username/password pair.

#### Env override: `CRONIQ_PASSWORD_LOGIN_ENABLED`

Set the env var to `false`/`no`/`off`/`0` to disable. Any other value
(including empty, garbage, or unset) leaves password login on — a typo
won't silently lock everyone out. The DSL block wins where set.

#### Boot-time guard

If `auth.password.enabled = false` **and** no OIDC provider can be
assembled from `CRONIQ_OIDC_*` env vars (or the `oidc {}` DSL block),
`croniq-server` refuses to start. A clear error message tells the
operator to either re-enable password login or finish the OIDC config —
quietly booting into a state where nobody can sign in would be a much
worse failure mode.

### `auth.totp.required` — enforced 2FA

```hcl
auth {
  totp {
    required true   # default: false
  }
}
```

When `true`, every password login must present a valid TOTP (or recovery)
code. The login UI reads this from `GET /v1/auth/config` and shows the code
field up-front, so an enforced login is a **single request**:
`POST /v1/auth/login` with `username` + `password` + `code`.

**Accounts without a confirmed TOTP secret are sent into inline enrolment**
at login: once the password is verified, `POST /v1/auth/login` returns
`{ "enrollment_required": true, "enroll_token": … }`, and the UI walks the
user through TOTP setup (`POST /v1/auth/login/enroll/totp/begin` then
`…/confirm`) before completing the sign-in. No one is locked out merely for
not having enrolled yet.

#### Env override: `CRONIQ_REQUIRE_TOTP`

Set to `true`/`yes`/`on`/`1` to enforce. Any other value (including empty,
garbage, or unset) leaves enforcement off — mirroring
`CRONIQ_PASSWORD_LOGIN_ENABLED`, a typo won't silently lock everyone out.
The DSL block wins where set.

#### Rollout

You can switch `required true` on at any time: password users who haven't
enrolled are guided through TOTP setup on their next sign-in (inline
enrolment, above) rather than locked out. Users can also enrol ahead of time
via **Settings → Two-factor authentication**. `croniq-server` logs a `WARN`
at boot whenever enforcement is on. SSO/OIDC and API-key callers are
unaffected — enforcement gates the password login flow only.

### JWT secret and stored TOTP secrets

Stored TOTP seeds are encrypted at rest with AES-256-GCM under a key derived
(HKDF-SHA256) from the **JWT secret**. The two are therefore coupled: change the
JWT secret and every stored TOTP secret becomes undecryptable.

The secret is resolved as `CRONIQ_JWT_SECRET` → `$DATA_DIR/jwt.secret`
(auto-created on first boot). Before 0.29.0, `pull_api { auth <value> }` came
first in that chain — it was never an auth on/off switch, its value was used
verbatim as the signing secret (issue #371). The directive was removed in
0.29.0, so **upgrading a deployment that relied on it** falls through to a
freshly generated `$DATA_DIR/jwt.secret`, silently rotating the wrap key
(issue #408).

A leftover `pull_api { auth … }` line is consequently a hard config error, not
an ignored line — the message names the migration. To upgrade:

1. Copy the value from `pull_api { auth <value> }` into the `CRONIQ_JWT_SECRET`
   env var (or write it to `$DATA_DIR/jwt.secret`).
2. Delete the `auth` line from the Croniqfile.
3. Start the server and check that `doctor` does not report
   `totp.secrets_undecryptable`.

If TOTP secrets have already become undecryptable, affected users get a bare
`500` from `POST /v1/auth/login` when they submit a code, and the server logs
`stored TOTP secret could not be unwrapped`. **Recovery codes still work** —
they are SHA-256 hashed, not wrapped — so the way back in is:

1. Restore the old secret via `CRONIQ_JWT_SECRET` if you still have it. That
   alone fixes everything.
2. Otherwise: sign in with a recovery code, then re-enrol under
   **Settings → Two-factor authentication**.
3. With no recovery code left either, an admin has to reset the user's second
   factor.

**Rotating `CRONIQ_JWT_SECRET` deliberately** invalidates stored TOTP secrets
the same way, so order the steps: relax enforcement
(`auth { totp { required false } }`) → rotate → have users re-enrol →
re-enable enforcement.

### Probing from the UI: `GET /v1/auth/config`

```jsonc
GET /v1/auth/config  →  200
{
  "oidc": {
    "enabled": true,
    "provider_name": "Authentik",
    "login_url": "http://localhost:4000/v1/auth/oidc/login"
  },
  "password": { "enabled": false },
  "totp": { "required": false }
}
```

The login UI hits this endpoint before any authentication happens and
uses the response to:

* hide the password form when `password.enabled === false`
* hide the SSO card when `oidc.enabled === false`
* show the 2FA code field up-front (single-request login) when
  `totp.required === true`
* show a "no sign-in method configured" blocker if both are off
  (mostly a defence-in-depth — the server refuses to boot in that
  state, so this branch is mainly for misconfigured load-balancers
  that hide one endpoint from the client)

The older `GET /v1/auth/oidc/config` continues to return its original
flat shape (`{enabled, provider_name, login_url}`) and is unaffected
by `auth.password.enabled` — keep using it if you have an external
probe that only cares about the SSO half.

### Demo-only seed flags

The docker entrypoint understands two opt-in env vars for the
marketing demo image. **Neither belongs in any production deployment.**

| Variable | Effect |
|---|---|
| `CRONIQ_DEMO_MODE=1` | Allows `CRONIQ_ADMIN_PASSWORD=admin`. Without it, the entrypoint refuses to start with a fixed `admin` password. |
| `CRONIQ_DEMO_MFA=1` | Pre-enables TOTP on the seeded admin and bakes the literal recovery code `123456` into all 10 slots. `admin/admin` then lands on the MFA prompt; typing `123456` completes login. The TOTP secret itself is still randomly generated, so a real authenticator code (if the secret is retrieved out-of-band) keeps working. |

`CRONIQ_DEMO_MFA=1` set on its own (without `CRONIQ_DEMO_MODE=1`)
emits a warning at first-boot init but still runs — the demo flag
isn't gated by the demo-mode guard so the warning is the only line
of defence against accidental production use.

Both flags are read by `croniq init` at first-boot only; they do
nothing on subsequent restarts where the database already exists.

## Configuration validation

Every path that loads a Croniqfile — server boot, `croniq-server doctor`, a
hot-reload, and the `croniq validate` CLI — runs the same semantic validation
and **fails closed** on any error (issue #402). Before this, only
`croniq validate` did, so the server quietly accepted configs that
`croniq validate` rejected: a duplicate job key dropped one of the jobs, a job
without a schedule was never scheduled, an unknown runner type compiled to
nothing, and `doctor` exited `0` for all of it.

What that means in practice:

- **Boot** — the server refuses to start and prints every error at once.
- **`doctor`** — one critical finding, exit `1`, so it works as a CI/pre-deploy
  gate.
- **Reload** — the reload is rejected and the previously running config stays
  active, exactly like a syntax error. `POST /v1/admin/reload-config` answers
  `422` with the message.

Unknown **directives** are errors too (issue #403), including inside
`server { }`, `pull_api { }`, `defaults { }`, `auth { }`, `observability { }`,
`mcp { }`, `policy { }`, `smtp { }` and `oidc { }` — with a did-you-mean
suggestion:

```console
$ croniq-server --config Croniqfile doctor
[CRITICAL] Croniqfile cannot be loaded
    invalid configuration:
      - unknown directive 'execution_retentionn' in `server { }` — did you mean 'execution_retention'?
    Run `croniq validate <Croniqfile>` for exact locations.
```

A typo used to be a silent no-op, which is the worst outcome for a setting whose
absence is invisible in the short term: a mistyped `execution_retention` left
run history growing forever and nothing misbehaved.

Two things are deliberately *not* boot failures:

- **Calendar problems** — a calendar whose rules don't compile, and a
  `calendar` reference that resolves to nothing, fail per job: the job loads
  **paused** with a `config_error`, under `policy { strict_calendars }`
  (issue #361). One broken calendar must not take the whole scheduler down.
- **Warnings** — e.g. an interval shorter than the runner poll cycle. They are
  logged at `WARN` and the config loads.

### Reload vs. restart

A reload (`--watch`, `SIGHUP`, `POST /v1/admin/reload-config`) re-reads the
whole file but only applies **jobs, calendars, triggers and the `policy { }`
flags**. Everything else — `server { }`, `pull_api { }`, `observability { }`,
`mcp { }`, `oidc { }`, `smtp { }`, `auth { }` and `alerts { }` — is read at boot
only.

Changing a boot-only setting and reloading is therefore a no-op, and used to be
a *silent* one: the reload reported success, so with `--watch` there was no
signal that part of the edit hadn't landed. This bites hardest in container
deployments, where a config-only change doesn't recreate the container and so
never restarts the process. Each changed boot-only setting is now named in a
`WARN` (issue #406):

```
WARN server.execution_retention changed in the Croniqfile (30d → 90d); this
     setting is applied at boot only and needs a server restart to take effect
```

They are also counted in the reload summary (`pending_restart=2`) and returned
by `POST /v1/admin/reload-config` as `pending_restart`, so a caller can report
"applied, with N settings pending restart" instead of a plain success. The
`alerts { }` block is compared as a fingerprint, never by value — a channel can
carry an HMAC signing key, which must not reach the log.

## Configuration diagnostics

croniq surfaces recommended-but-missing configuration in three places, all
backed by the same checks:

- **At boot** — each finding is logged (`WARN`/`ERROR`) when the server starts.
- **`croniq-server doctor`** — an offline preflight that loads the Croniqfile +
  env, prints the report, and exits non-zero on any critical finding. It does
  not bind ports or open the database, so it is safe to run before a deploy:
  ```sh
  croniq-server --config Croniqfile doctor
  ```
- **`GET /v1/system/diagnostics`** — admin-only JSON, consumed by the
  Settings → System panel in the dashboard.

Current checks:

| id | severity | meaning |
|---|---|---|
| `email.delivery` | warning | No SMTP configured — invitation / password-reset links are returned in the API/UI response only and must be delivered manually. |
| `email.smtp_feature_missing` | critical | `CRONIQ_SMTP_*` is set but this build lacks the `smtp` feature, so mail is silently dropped. |
| `links.app_url` | warning | No public base URL pinned (`server { app_url }` / `CRONIQ_APP_URL`); links are derived from request headers and the public password-reset link falls back to localhost on a directly-exposed server. |
| `totp.enforced_without_enrollment` | info | Enforced 2FA (`require_totp`) is on and one or more active users have no confirmed TOTP secret — they will be walked through [inline enrolment](#authtotprequired--enforced-2fa) at their next sign-in. Expected right after switching enforcement on; nobody is locked out. (Live surfaces only; `doctor` can't evaluate it offline.) |
| `totp.secrets_undecryptable` | critical | One or more confirmed TOTP secrets no longer decrypt with the active JWT secret, so those users get a `500` from `POST /v1/auth/login` when they submit a code. See [JWT secret and stored TOTP secrets](#jwt-secret-and-stored-totp-secrets). (Live surfaces only.) |
| `retention.unbounded_history` | info | Neither `server { execution_retention }` nor any `keep_last` is set, so run history grows without bound. See [Data retention](#data-retention). Informational by design — it never makes `doctor` exit non-zero. |

Findings report posture only — never secrets.

A Croniqfile that fails to load at all is reported by `doctor` as a single
critical finding (and exits `1`) — see [Configuration
validation](#configuration-validation).

## Alert rule overrides

Alert rules and channels are defined in the Croniqfile (`alert {}` / channel
blocks) and are read-only at runtime. **Operational overrides** let an admin
temporarily change a rule's behaviour during an incident without editing the
Croniqfile, redeploying, or losing the original definition. They are gated by
the admin-only `alerts:write` scope and every set-action requires a `note`
(captured at write time, surfaced in the audit log).

Three intents, one per rule — they are not composable; setting one replaces any
existing override for that rule:

| action | endpoint | effect |
|---|---|---|
| snooze | `POST /v1/alerts/rules/{name}/snooze` | suppress the rule until `until`; auto-clears at that instant |
| disable | `POST /v1/alerts/rules/{name}/disable` | suppress the rule open-ended (or until `expires_at`) |
| throttle | `POST /v1/alerts/rules/{name}/throttle` | **replace** the DSL throttle window with a new duration |

Inspect with `GET /v1/alerts/rules/{name}/override`, clear with
`DELETE …/override`, and see all active overrides inline on
`GET /v1/alerts/config`. Expired overrides are inert immediately and swept by
the watchdog; overrides for rules that no longer exist in the Croniqfile are
pruned at boot.

### When to override vs. when to edit the Croniqfile

```
Is this a permanent change to how the rule should behave?
├─ Yes → edit the Croniqfile (alert {} block) + redeploy. Overrides are
│        for temporary, incident-scoped deviations only.
└─ No (temporary / incident-scoped) →
   ├─ Planned maintenance window with a known end time?
   │     → snooze (until = end of the window). Auto-clears, no follow-up.
   ├─ Rule is firing on a known false positive you're actively debugging?
   │     → disable (set expires_at if you have a deadline; otherwise
   │       open-ended, but clear it when done — open-ended disables are
   │       the thing most likely to be forgotten).
   └─ Rule is correct but too noisy right now (flapping dependency)?
         → throttle with a LONGER window. Note this REPLACES the DSL
           throttle rather than taking the min — the operator use-case is
           "this is too aggressive, widen it", so the override wins
           outright. To go back to the DSL window, clear the override.
```

Rule of thumb: if you'd want the change to survive a redeploy, it belongs in
the Croniqfile, not an override.

## Orphaned claims (issue #374)

A `claimed` execution whose runner process vanished is recovered by
complementary mechanisms — no operator action needed:

- **Runner restart (same `runner_id`, new `instance_id`)**: the first poll of
  the new process takes the identity over and the old session's claims are
  requeued immediately. Each takeover logs a warning and records a
  `runner.takeover` audit event (target = the runner id). The deposed
  instance id is fenced — if the old process is actually still alive
  (duplicate deployment sharing one `runner_id`), its polls get
  `409 Conflict` and the SDK exits after its conflict streak.
  Consequence for rolling deploys with overlap: the draining old instance's
  in-flight claims are requeued at takeover and may run twice — prefer
  stop-before-start, or give each replica its own `runner_id`.

  **Identity flapping (duplicate deployment + restart policy)**: the fencing
  above converges to a stable winner only when the fenced loser *stays*
  exited. Under a container restart policy (`restart: always` & co.) the
  loser comes back with a fresh `instance_id`, immediately takes the
  identity over, fences the other process, which crashes out, restarts, and
  takes over again — an endless alternating takeover ping-pong. Jobs keep
  running, so this is easy to miss, but every switch requeues the loser's
  claims (churn, possible double runs). The server detects it: **3 or more
  takeovers of the same `runner_id` within 10 minutes** log a
  `runner identity flapping` warning and record a `runner.identity_flapping`
  audit event (throttled to once per window while the flapping lasts). If
  you see either, two live deployments almost certainly share one
  `runner_id` — give each replica its own `runner_id` (or stop one). A
  steady stream of `runner.takeover` events with no matching deploys points
  at the same root cause.
- **Stale-claim reaper** (watchdog sweep, every 30 s): any execution still
  `claimed` after the job `timeout` (default `5m`) plus a grace window of
  `max(2 × lease_ttl, 120 s)` is requeued with the same attempt number,
  regardless of runner liveness — this also catches claims orphaned across a
  server restart. Claims that a live runner still reports inflight are exempt.
  Each reap logs a `watchdog: requeued stale claimed execution` warning and an
  `execution.stale_claim_requeued` audit event; recurring reaps for the same
  job are the signal to investigate that runner's stability.
- **Queued-reconcile sweep** (same watchdog cadence): a row that is `queued`
  in the store but missing from the in-memory dispatch queue (e.g. a requeue
  that could not be re-enqueued right away, or a server restart between store
  write and dispatch) is re-enqueued. If the row's job no longer exists in
  the DSL or the store, the execution is cancelled instead, with an
  `execution.stranded_queued_cancelled` audit event.

The claim sweeps list the **oldest claims first**, so with more than 500
concurrent in-flight claims the overflow only defers the newest claims to a
later sweep — the orphaned/SLA-breached claims the sweeps target are always
inside the window (no starvation). The reconcile sweep likewise processes up
to 500 rows per tick, oldest-due first; a larger backlog drains across ticks.

A `singleton` job wedged by such an orphan therefore self-heals within one
sweep after `timeout + grace` at the latest.

### Watchdog metrics

The frequency of these recovery actions is the operator signal, so `/metrics`
exposes them as cumulative Prometheus counters (process lifetime):

- `croniq_watchdog_requeued_total{reason="dead_runner"|"stale_claim"|"reconciled"}`
  — executions requeued by the dead-runner sweep + inline takeover, the
  stale-claim reaper, and the queued-reconcile sweep respectively. A rising
  `dead_runner`/`stale_claim` rate means unstable runners.
- `croniq_watchdog_cancelled_total{reason="queue_ttl"|"stranded"}` — queued
  executions cancelled on `queue_ttl` expiry, and stranded rows cancelled
  because their job was deleted with work still in flight.
- `croniq_watchdog_sla_missed_total` / `croniq_watchdog_missed_fires_total` —
  `job_sla_missed` / `job_missed_fire` alerts fired by the sweep.

## Data retention

The server's watchdog runs a housekeeping sweep every 30 s. Two sources of
row growth are capped there; both apply to the SQLite and Postgres backends.

### Dead letters

Per-job `dead_letter { retention <dur> }` (default `30d`). Each dead-letter row
is stamped with an `expires_at = created_at + retention` at write time; the
sweep deletes rows past their `expires_at`. Set `retention 0` (or
`dead_letter { enabled false }`) to disable.

Rows without an `expires_at` are **never auto-purged** — deliberately so. That
covers `retention 0` rows and the one-time cohort backfilled by migration 009
(orphaned `dead` executions from before the #104 fix), where stamping a
retention retroactively would have silently deleted triage data nobody chose a
TTL for. If such rows have piled up, clear them explicitly: bulk-delete in the
Dead Letters UI, `DELETE /v1/dead-letters/{id}`, or
`POST /v1/dead-letters/bulk-delete` (optionally scoped to a `job_key`).

### Terminal executions (issue #344)

Non-`ephemeral` executions are persisted for run history and would otherwise
accumulate forever. Two **opt-in** knobs cap them; both prune terminal
executions (`completed` / `failed` / `cancelled`) together with their logs, and
both **exclude `dead` executions** (those follow dead-letter retention above).

- **Age sweep** — `server { execution_retention <dur> }` (e.g. `30d`). Prunes
  executions whose `completed_at` is older than the cutoff.
- **Per-job cap** — `keep_last <N>` in `defaults { }` or a `job { }` block.
  Keeps the newest `N` terminal executions per job and prunes the rest. Applies
  on top of the age sweep.

Both are **off by default** — an upgrade never silently deletes run history.
Deletions run in bounded batches, so the first sweep after enabling retention
on a large existing database drains the backlog over several ticks rather than
in one long-locking statement (this matters most for SQLite's whole-database
write lock). An invalid or zero `execution_retention` is ignored (pruning
stays disabled) and logged at boot. Changes to these knobs take effect on
server restart — a hot-reload parses them and then warns that they are pending
a restart (see [Reload](#reload-vs-restart) below). Once pruned, an execution
and its logs are gone; dashboards, `/metrics` aggregates, and the UI run
history reflect only the retained window.

With neither knob set, `croniq-server doctor` reports
`retention.unbounded_history` as an informational finding. It never turns the
exit code non-zero — keeping history forever is a legitimate choice — it only
makes sure the decision is visible.

#### Pruning caps growth; it does not shrink the file (issue #404)

Retention deletes rows. It does **not** return disk space to the filesystem:

- **SQLite** — deleted pages go on the freelist and are reused by later
  writes, so the database stops growing, but `croniq.db` stays at its
  high-water mark. Reclaiming that space needs an explicit `VACUUM`, which
  rewrites the whole database: budget roughly the current DB size in free
  space, and expect an exclusive lock for the duration (no writes, so no job
  state changes while it runs). Croniq never runs it for you, and no migration
  sets `PRAGMA auto_vacuum` — that pragma only takes effect on a database where
  it was set *before* the tables were created, so on an existing database it
  needs a full `VACUUM` anyway.
- **PostgreSQL** — autovacuum makes the space reusable within the table;
  returning it to the OS needs `VACUUM FULL`, with the same caveats.

This matters because enabling retention is usually a reaction to "the database
got bigger than I expected". Enabling it, restarting, and waiting for the sweep
leaves the file at exactly its previous size — which is expected, not a sign
that retention isn't working. To confirm it *is* working, watch the row counts
(or SQLite's `PRAGMA freelist_count` growing) rather than the file size.
