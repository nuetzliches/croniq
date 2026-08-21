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

## HTTP hardening

### CORS

The server emits CORS headers only when a public app URL is configured
(`server { app_url "…" }` in the Croniqfile, or `CRONIQ_APP_URL`). In the
standard setup — croniq-server serves both the API and the dashboard SPA,
as in the official Docker image — everything is same-origin, no CORS headers
are needed, and none are sent; browsers enforce the same-origin policy
unaided.

When an app URL *is* configured, exactly its origin (scheme + host + port)
is allowed, with the methods and headers the dashboard uses (`GET`, `POST`,
`PUT`, `PATCH`, `DELETE`; `Authorization`, `Content-Type`). There is no
wildcard and `Access-Control-Allow-Credentials` is never set — cross-origin
authentication is Bearer-header only. (The refresh cookie introduced by #454 is
`SameSite=Strict` and same-origin only, so it never participates in a
cross-origin request either; see *Where the dashboard keeps its tokens*.)
Consequences worth knowing:

- A dashboard built with `VITE_API_URL` pointing at a server on a different
  origin needs that server to have `app_url` set to the dashboard's URL, or
  browser calls will be blocked.
- Non-browser clients (runners, the CLI, curl, SDKs) are unaffected — CORS
  is a browser-side read gate, not authentication.
- `server.app_url` is boot-only (see *Reload vs. restart*): changing it
  requires a restart, and the CORS allowlist follows suit.

### Security headers

Every response — API, dashboard, and `/mcp` — carries:

| Header | Value |
|---|---|
| `X-Content-Type-Options` | `nosniff` |
| `X-Frame-Options` | `DENY` |
| `Referrer-Policy` | `no-referrer` |
| `Content-Security-Policy` | see below |

The CSP is scoped to what the dashboard bundle actually needs: `default-src
'self'`, `script-src 'self' 'wasm-unsafe-eval'` (the schedule builder runs
the DSL parser as WebAssembly), `style-src 'self' 'unsafe-inline'` (React
style attributes), `img-src 'self' data:`, `connect-src 'self'`,
`frame-ancestors 'none'`, `object-src 'none'`, `base-uri 'self'`,
`form-action 'self'`. The exact value and per-directive rationale live in
[`hardening.rs`](../crates/croniq-server/src/api/hardening.rs).

`Strict-Transport-Security` is **not** set by croniq-server: it does not
terminate TLS itself, and HSTS sent over plain HTTP is ignored by browsers
anyway. If you terminate TLS in front of Croniq (reverse proxy, ingress,
load balancer), add HSTS there, e.g.
`Strict-Transport-Security: max-age=31536000` once you are confident the
host will stay HTTPS-only.

### Where the dashboard keeps its tokens

A password or SSO login mints two credentials with very different lifetimes:
an access token (a stateless JWT, one hour) and a refresh token (opaque,
seven days, stored hashed server-side). Until #454 the dashboard kept both in
`localStorage`, which meant any XSS — or a compromised npm dependency
executing at runtime — could lift the refresh token and hold the account for a
week, surviving reloads and outliving the access token that `token_generation`
makes cheap to revoke.

In the standard setup — croniq-server serving both the API and the dashboard,
as the official Docker image does — the split is now:

| Credential | Where it lives | Reachable from JavaScript |
|---|---|---|
| Access token | Memory only (never persisted) | Yes, by design — it is sent as `Authorization: Bearer …` |
| Refresh token | `croniq_refresh` cookie: `HttpOnly; SameSite=Strict; Path=/v1/auth` | **No** |

Consequences worth knowing:

- **A reload starts with no access token.** The dashboard silently calls
  `POST /v1/auth/refresh`, which the browser answers with the cookie, and gets
  a fresh access token. This is why a reload briefly shows a spinner rather
  than the login page.
- **A session now survives past the access token's hour.** A 401 mid-session
  triggers a refresh and a retry instead of dropping the user at the login
  screen.
- **Logging out is a server round-trip.** Clearing a cookie does not revoke
  the token behind it, so `POST /v1/auth/logout` revokes it server-side and
  clears the cookie in the same response.
- **There is still no CSRF surface.** `SameSite=Strict` means the cookie is
  never attached to a cross-site request, only `/v1/auth/refresh` accepts it,
  and the token it mints goes into a response body that a foreign page cannot
  read (CORS is origin-locked — see *CORS* above). Every other API call
  authenticates with an `Authorization` header, so no ambient authority exists
  anywhere else.
- **`Secure` is set only when the server can tell the page is on HTTPS**
  (`Origin`, `X-Forwarded-Proto`, or an `https://` `app_url`). Browsers never
  send a `Secure` cookie back over plain HTTP, so setting it on a plain-HTTP
  deployment would lock everyone out rather than harden anything. Terminate
  TLS in front of Croniq and the flag appears by itself.
- **Non-browser clients are unaffected.** The CLI, curl, the SDKs and any
  scripted flow keep receiving `refresh_token` in the login response body and
  keep passing it in the refresh request body. Cookie delivery is opt-in per
  request (`"refresh_cookie": true`), and a cookie-sourced refresh is the only
  thing that omits the body field.

#### Cross-origin dashboards (`VITE_API_URL`)

A `SameSite=Strict` cookie cannot reach a dashboard served from a different
origin than the API, so such a build has to keep the refresh token in
`localStorage` — with exactly the exposure described above. Because that is a
trade rather than a default, `ui/vite.config.ts` refuses to build a
`VITE_API_URL` bundle unless it is acknowledged:

```
VITE_API_URL=https://api.example.com \
VITE_ALLOW_LOCALSTORAGE_REFRESH=1 \
npm run build
```

Without the second variable the build fails with an explanation. If you can
serve the dashboard from croniq-server itself instead, that is the stronger
option and needs no flags at all. (Local development is unaffected: `npm run
dev` proxies `/v1` through the Vite dev server, so the browser sees a single
origin and gets the cookie.)

### Keep `/metrics` on an internal interface

The Prometheus endpoint is unauthenticated by design (the standard scrape
pattern) and lives on its own opt-in listener
(`observability { metrics { listen :9900 } }`). It exposes job keys, queue
depths, and runner names — operational data, not secrets, but nothing that
belongs on the public internet either. Bind it to an internal address
(`listen "127.0.0.1:9900"` or an internal network interface) or restrict it
with firewall/network-policy rules; a bare port form like `listen :9900`
binds `0.0.0.0`.

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

### PostgreSQL TLS (`CRONIQ_PG_SSLMODE`, `CRONIQ_PG_ROOT_CERT`)

Applies to the Postgres backend only (built with the off-by-default
`croniq-store/postgres` feature).

Before issue #431 the driver was handed `NoTls` unconditionally. Against a local
database that is harmless; against a remote one the connection password and
every row the auth tables return — password hashes, wrapped TOTP secrets,
API-key hashes — crossed the network in cleartext. The connector is now
rustls-based (no OpenSSL, no C toolchain).

The mode is resolved highest-first from:

1. `sslmode=` in the connection string (libpq spelling, so existing connection
   strings keep working);
2. `CRONIQ_PG_SSLMODE`;
3. the default — see the breaking-change note below.

| Mode | Behaviour |
|---|---|
| `disable` | No TLS. Sane only for a unix socket or a trusted local host. |
| `prefer` | TLS when the server offers it, cleartext when it does not. |
| `require` (also `verify-ca`, `verify-full`) | TLS or no connection at all. |

**Default:** `require` when every host in the connection string is remote,
`prefer` when it is loopback or a unix socket.

**Certificate verification is always on when TLS is used.** There is no
"encrypt but don't check who you're talking to" mode — it stops none of the
attacks this closes. This is stricter than libpq, where plain `require` skips
verification; a Croniq `require` behaves like libpq's `verify-full`.

Roots come from the platform trust store plus the Mozilla bundle.
`CRONIQ_PG_ROOT_CERT` points at a PEM file of additional CAs — an internal PKI,
or Amazon RDS's `rds-ca-…` bundle. An unreadable or empty file is a hard error
rather than a silent fallback, so a typo surfaces at boot instead of as a
confusing handshake failure.

> **Breaking-ish.** A remote Postgres that does not speak TLS, or presents a
> certificate from a CA the host does not trust, now fails to connect where it
> previously connected in cleartext. The connection error names both escape
> hatches. To keep the old behaviour: `CRONIQ_PG_SSLMODE=prefer` (or `disable`).
> To fix it properly: enable TLS on the server, and add its CA via
> `CRONIQ_PG_ROOT_CERT` if it is privately issued.

### Shell-runner job environment (`CRONIQ_RUNNER_ENV_PASSTHROUGH`)

`croniq-shell-runner` used to copy its entire process environment into every
`runner shell {}` / `runner exec {}` job, so any job author could read the
runner's own `CRONIQ_API_KEY`. Since issue #431 jobs inherit an allowlist
instead: `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `TMPDIR`, `TZ`, the `LC_*`
/ `LANG` locale set, and the Windows variables a subprocess cannot start without
(`SYSTEMROOT`, `COMSPEC`, `PATHEXT`, `TEMP`, `TMP`, `APPDATA`, `PROGRAMFILES`,
…). `env {}` in the Croniqfile is applied afterwards and still overrides any of
them.

`CRONIQ_*` is never inherited implicitly — that prefix is where the runner's own
credentials live.

`CRONIQ_RUNNER_ENV_PASSTHROUGH` widens the list, as a comma-separated value:

| Value | Effect |
|---|---|
| `MY_TOKEN,OTHER_VAR` | Inherit these names in addition to the allowlist. Naming a `CRONIQ_*` variable explicitly *does* pass it through — that is a deliberate operator act. |
| `*` | Inherit the runner's whole environment, i.e. the pre-#431 behaviour. `CRONIQ_*` is still withheld: a blunt wildcard must not hand out the runner's credentials. |

> **Breaking-ish.** A job that silently relied on an inherited variable —
> `AWS_ACCESS_KEY_ID`, `JAVA_HOME`, a proxy setting — stops seeing it. Declare
> it in the job's `env {}` block, or add it to
> `CRONIQ_RUNNER_ENV_PASSTHROUGH`.

In the same change, a `user` directive the runner cannot honour now **fails the
job** instead of running it as the runner's own user. A non-numeric `user` on
unix, or any `user` off unix, was previously logged and ignored — which meant
the job ran with *more* privilege than it asked for, possibly root. Set a
numeric uid, or drop the directive and run the runner process itself as the
desired user.

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
(auto-created on first boot).

`jwt.secret` is created restricted to the account that runs the server: mode
`0600` on Unix, and since issue #431 a single non-inherited ACE on Windows
(applied with `icacls /inheritance:r /grant:r`). Previously the Windows branch
was a plain write, so the file inherited the data directory's ACL — typically
`Users:(RX)` under `C:\ProgramData`, i.e. readable by every local account. Since
this one file both signs every token and derives the TOTP at-rest key, that was
the file in the tree that least deserved a permissive ACL. If `icacls` cannot
run, the server fails to start rather than writing the key unprotected; supply
`CRONIQ_JWT_SECRET` instead and no file is written at all. Before 0.29.0, `pull_api { auth <value> }` came
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

### Session invalidation: `token_generation` (issue #431)

Access tokens are stateless JWTs valid until `exp` — an hour by default. That
used to mean a password change, a password reset, or deactivating an account
did **not** end sessions already in progress: refresh was correctly blocked
(it re-checks `is_active`), but every access token minted beforehand kept
working for up to an hour. "I reset the password to lock the attacker out" is a
reasonable operator expectation and it did not hold.

Each user row now carries a `token_generation` counter (migration `025`). It is
stamped into every access token as a claim and compared against the row on
every JWT-authenticated request, and it is incremented on exactly three events:

| Event | Endpoint |
|---|---|
| Password change | `POST /v1/users/me/change-password` |
| Password reset completed | `POST /v1/auth/password-reset/confirm` |
| User deactivated | `PATCH /v1/users/{id}` with `is_active: false` |

Everything else — profile edits, role changes, `PATCH /v1/users/me` — leaves the
counter alone. Signing someone out is a real cost, so it is spent only where the
credential itself changed or the account was disabled; a role change already
propagates on the next refresh.

Operational notes:

* **Cost.** One primary-key lookup per JWT-authenticated request. The API-key
  and PAT paths already did store I/O per request; this brings the JWT path in
  line. That is the price of making a stateless token revocable.
* **API keys and PATs are unaffected.** Neither carries the claim — both are
  already re-checked against their own revocation columns on every request.
* **Rolling restarts are safe.** Tokens minted by an older binary carry no
  claim and are read as generation `0`, which is what every existing row was
  backfilled to. Nobody is signed out by the upgrade itself; the first bump on
  a given account invalidates its older tokens along with the rest.
* **A deleted user's tokens stop working immediately** — no row means no
  generation to match.

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

### Declaring API clients in the environment

A deployment that renders its environment before the stack comes up can pin
its machine credentials by value, including scoped ones:

```yaml
environment:
  # The runner only needs the pull protocol.
  CRONIQ_API_CLIENT_RUNNER_KEY: croniq_...
  CRONIQ_API_CLIENT_RUNNER_SCOPES: work:poll,work:ack,work:renew
  # The producer only needs to fire jobs.
  CRONIQ_API_CLIENT_PRODUCER_KEY: croniq_...
  CRONIQ_API_CLIENT_PRODUCER_SCOPES: jobs:trigger
```

Boot the stack and both clients exist with those scopes. Nothing has to be
created through the API afterwards, and no credential has to be copied back
into the deployment.

`<NAME>` is `[A-Z0-9_]`, lowercased with `_` → `-`, so
`CRONIQ_API_CLIENT_RUNNER_POLL_KEY` declares the client `runner-poll`. Every
key variable also accepts the `_FILE` form (`CRONIQ_API_CLIENT_RUNNER_KEY_FILE`).

For a single admin credential the short form still works and names the client
`default`:

```
CRONIQ_API_KEY=croniq_...          # or CRONIQ_API_KEY_FILE
CRONIQ_API_KEY_SCOPES=...          # optional; admin when creating
```

`CRONIQ_INIT_API_KEY` remains an alias for the same thing. It is deprecated
but not going away.

**Leaving the scopes variable unset is not the same as setting it to `admin`.**
A `default` client that does not exist yet is created with `admin`, which is
what the bare variable has always seeded. But an *existing* client keeps the
scopes it has: if you narrowed `default` in the dashboard, a reconcile with no
`CRONIQ_API_KEY_SCOPES` set leaves that alone rather than putting it back. To
change scopes from the environment, name them.

**Scopes are mandatory for a named client.** Omitting them is an error rather
than a fall-back to `admin` — silently granting the wildcard is the problem
this feature exists to remove. An unknown scope (`job:reed`) is also a boot
error: it would otherwise produce a credential that authorises nothing and
fails at first use, in some other service, far from the file that caused it.

**What is fatal at boot, and what is only logged.** A malformed *current*
declaration stops the server: a key that does not begin with `croniq_`, a
named client without scopes, an unknown scope, or the same client declared
twice with different values. All of those are a declaration written wrong, and
booting past one means running without the credential that was asked for.

Two things are logged and skipped instead:

- A `CRONIQ_API_CLIENT_*` variable with an unrecognised suffix. Croniq does not
  read it, so refusing to start would claim the whole namespace for a variable
  it has no use for. The typo that would actually change behaviour — a
  misspelled `_SCOPES` — still fails, as the missing-scopes error above.
- A `CRONIQ_INIT_API_KEY` whose value is not a `croniq_` key. The deprecated
  variable behaved this way before v0.34, and a leftover placeholder from an
  old template should not turn a version bump into a restart loop.

#### What the reconciler will and will not do on its own

| Situation | Without `CRONIQ_API_KEY_RECONCILE=1` | With it |
|---|---|---|
| Client does not exist | created | created |
| Store matches the declaration | no-op | no-op |
| Declared key differs | logged, **not** rotated | rotated (see grace window) |
| Declared scopes differ | logged, **not** changed | updated |
| No scopes variable set | no-op | no-op (the stored scopes stand) |
| Client exists but is API-owned | logged, ownership unchanged | ownership moves to the environment |

Creating a client is additive — it cannot break a credential that is already
working — so it needs no flag. Everything that rewrites existing state does,
because an env value changed by accident should not be able to take a running
deployment offline.

#### Ownership

A client the environment created is stored with `managed_by: "env"`. From then
on the environment is its source of truth, and the API refuses to edit it,
delete it, or mint keys for it — each with a 409 naming the variable to change
instead. The dashboard shows those clients with an `env-managed` badge and
disabled controls.

Without that rule a scope change made in the dashboard would survive until the
next reconcile and then revert, with nothing linking the two events.

Ownership never moves silently: a client that already exists as `managed_by:
"api"` — one created in the dashboard, or seeded by `croniq init --api-key` —
stays API-owned until an operator sets `CRONIQ_API_KEY_RECONCILE=1`. Upgrading
a deployment whose client names happen to collide with new declarations
therefore changes nothing on its own. To hand a client back to the API, remove
its declaration and restart.

#### Rotating without a restart

The direct environment of a running process cannot be changed from outside, so
`CRONIQ_API_CLIENT_..._KEY` only takes effect at boot. The `_FILE` form does
not have that limit: the file can be rewritten under a live process, which is
what a Kubernetes Secret volume or a Vault sidecar does.

Two triggers re-read it — both explicit:

```bash
kill -HUP <pid>
# or
curl -X POST -H "Authorization: ApiKey $ADMIN_KEY" \
  "$CRONIQ_URL/v1/admin/reload-config"
```

```json
{
  "applied": true,
  "credentials": [
    { "client": "runner", "action": "rotated" },
    { "client": "producer", "action": "unchanged" }
  ]
}
```

`?dry_run=true` reports the same `credentials` block without writing anything.

The `--watch` file watcher deliberately does **not** re-read credentials. It
fires on every write, including the partial one a secret manager makes halfway
through replacing a file, and installing whatever that file happened to contain
at that instant is not a risk worth taking for a convenience.

### Rotating an API key without downtime

`CRONIQ_API_KEY` (or any `CRONIQ_API_CLIENT_<NAME>_KEY`) plus
`CRONIQ_API_KEY_RECONCILE=1` rotate a declared client's key from
configuration. Two things are worth knowing before relying on it.

**The direct env var only rotates at boot.** The environment of a running
process cannot be changed from outside — setting a new value in Compose or
a Deployment means a new container, not a new value in the old one. To
rotate a *running* server, use the `_FILE` form and point it at a mounted
secret (a Kubernetes Secret volume, a Vault/Infisical sidecar target, a
bind-mounted file). That file *does* change under a live process, and
`SIGHUP` or `POST /v1/admin/reload-config` re-reads it — see
[Declaring API clients in the environment](#declaring-api-clients-in-the-environment).

**The superseded key is retired, not revoked.** A rotation installs the new
key and stamps the old one with `expires_at = now + CRONIQ_API_KEY_ROTATION_GRACE`
(default `15m`). It keeps authenticating until then.

That window exists because the server is not the only holder of the
credential. A runner in another container has the old value in memory: the
SDK reads its API key once, at construction, and never re-reads it, so a
running process cannot pick up the new key at all. Revoking instantly
therefore does not produce a brief blip — every poll from that runner fails
until something restarts it. The grace window covers the secret-volume
refresh and the consumer rollout.

A runner no longer retries a rejected credential forever — it exits, so a
supervisor can restart it with the new key. That changes "never recovers" to
"recovers on restart", but it does not remove the need for the window: it puts
a clock on it. See [How long a runner survives a rejected
key](#how-long-a-runner-survives-a-rejected-key).

Inspect the handover:

```bash
curl -H "Authorization: ApiKey $ADMIN_KEY" \
  "$CRONIQ_URL/v1/api-keys?client_id=$CLIENT_ID"
```

```json
[
  { "key_id": "9f3…", "key_prefix": "croniq_a1b2c", "created_at": "2026-08-20T09:00:00Z" },
  { "key_id": "1c8…", "key_prefix": "croniq_0f9e8", "created_at": "2026-05-02T11:20:00Z",
    "expires_at": "2026-08-20T09:15:00Z" }
]
```

The row carrying `expires_at` is the outgoing key. Once that instant passes
it stops working; the row stays for audit.

**A leaked key is a different problem.** The grace window deliberately keeps
the old value alive, so it is the wrong tool. Either:

- rotate normally, then end the old key immediately with its `key_id` from
  the listing above:
  ```bash
  curl -X DELETE -H "Authorization: ApiKey $ADMIN_KEY" \
    "$CRONIQ_URL/v1/api-keys/1c8…"
  ```
- or set `CRONIQ_API_KEY_ROTATION_GRACE=0s`, which restores the pre-v0.34
  behaviour of revoking every superseded key as part of the rotation. Use
  this where a policy requires that revocation be instant.

Note that revoking a key alone does not un-leak an env-declared credential:
if the declared value is unchanged, the next reconcile with
`CRONIQ_INIT_API_KEY_RECONCILE=1` installs it again. Change the declared
value first, then rotate.

### How long a runner survives a rejected key

A runner that gets `401` on `POST /v1/work/poll` counts it. After
`max_consecutive_auth_failures` in a row — **default 3** — it stops with a
fatal authentication error instead of polling on. The credential is read once
at construction and never re-read, so retrying cannot clear a rejection;
exiting non-zero is what lets a supervisor restart the process and pick up the
new key.

There is no backoff on `401`, so the budget is spent at the poll interval:

| poll interval | budget | time to exit |
|---|---|---|
| 5s (default) | 3 (default) | ~10s |
| 5s | 10 | ~45s |
| 30s | 3 | ~60s |

The counter resets on any successful poll and on any other error — a 5xx or a
timeout says nothing about whether the key is valid.

**This assumes the runner is supervised.** Under Kubernetes, a Compose
`restart:` policy, or systemd, the exit is the mechanism: the process dies, the
supervisor restarts it, construction re-reads the environment, and the new key
takes effect. Crash-looping for as long as the credential is genuinely gone is
the intended, visible behaviour.

**A runner with nothing to restart it needs a different setting.** A bare
process, a `nohup`-ed script, a developer's laptop — there the default turns a
10-second credential hiccup into a runner that is simply gone, with no
schedule running and nothing reporting why beyond a log line. Either put it
under a supervisor, or raise the budget so the process outlives the handover:

```rust
CroniqRunner::builder(url, "worker-1")
    .max_consecutive_auth_failures(60)   // ~5 min at the default interval
    .build();
```

The equivalent option exists in all six SDKs (`max_consecutive_auth_failures`,
`maxConsecutiveAuthFailures`, `MaxConsecutiveAuthFailures`). Note that `0` does
**not** disable the budget: the threshold is checked as "streak >= limit", so
the first `401` trips it immediately — the opposite of what you want. Use a
large finite value.

**Sizing it against the rotation grace.** A runner only sees `401` once its key
has actually stopped working, which during a normal rotation means the grace
window closed before the rollout reached it. So the two settings answer
different halves of the same question: `CRONIQ_API_KEY_ROTATION_GRACE` should
cover how long a full consumer rollout takes, and the auth budget only has to
cover the gap between a runner's key dying and its supervisor restarting it. If
you find yourself raising the budget to survive routine rotations, the grace
window is the setting that is too small.

### Demo-only seed flags

The docker entrypoint understands two opt-in env vars for the
marketing demo image. **Neither belongs in any production deployment.**

| Variable | Effect |
|---|---|
| `CRONIQ_DEMO_MODE=1` | Marks the deployment as the local demo stack. It no longer weakens the password rules: `CRONIQ_ADMIN_PASSWORD=admin` is refused with or without it, because `croniq init` enforces the same 8–72 byte policy as every other password path (issue #428). The demo stack ships `demo-admin`. |
| `CRONIQ_DEMO_MFA=1` | Pre-enables TOTP on the seeded admin and bakes the literal recovery code `123456` into all 10 slots. `admin/demo-admin` then lands on the MFA prompt; typing `123456` completes login. The TOTP secret itself is still randomly generated, so a real authenticator code (if the secret is retrieved out-of-band) keeps working. |

`CRONIQ_DEMO_MFA=1` set on its own (without `CRONIQ_DEMO_MODE=1`)
emits a warning at first-boot init but still runs — the demo flag
isn't gated by the demo-mode guard so the warning is the only line
of defence against accidental production use.

Both flags are read by `croniq init` at first-boot only; they do
nothing on subsequent restarts where the database already exists.

#### Demo mode cannot be exposed to the network (issue #431)

The credentials above are published in this repository, so the only thing
protecting a demo instance is that nobody else can reach it. That used to be a
comment in `docker-compose.yml`; it is now enforced in two places:

* **`croniq-server` refuses to start** with `CRONIQ_DEMO_MODE=1` if `--listen`
  (or `--metrics`) resolves to anything other than a loopback address. The
  default `--listen :4000` means `0.0.0.0`, so a demo-mode server started
  directly on a host now fails with an explanatory error instead of publishing
  `admin/demo-admin` to every interface. Bind `127.0.0.1:4000`, or drop
  `CRONIQ_DEMO_MODE` and configure real credentials.
* **`docker-compose.yml` publishes to `127.0.0.1` only.** A container has its
  own network namespace and *must* bind `0.0.0.0` for a published port to reach
  it at all, so the in-process rule cannot apply there — `docker-entrypoint.sh`
  sets `CRONIQ_DEMO_CONTAINER_BIND=1` to say so, and the server logs a warning
  instead of refusing. What decides exposure for the compose stack is the
  host-side publish. **Removing the `127.0.0.1:` prefix from the `ports:`
  entries re-exposes the demo credentials** — on a cloud VM, to the internet.

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

## Which timezone applies where

Three things carry a zone, and they are resolved independently. Getting this
wrong changes *when* jobs fire, so croniq says which zone it ended up with
rather than leaving it implied.

| what | zone it is read on | resolution order |
|---|---|---|
| a job's wall-clock schedule (`every day at 02:00`, `every weekday at …`, `every Nth of month at …`) | the job's | schedule option > job-level `timezone` > `defaults { timezone }` > UTC |
| a job's `window HH:MM..HH:MM` directive, and `not_before` / `not_after` | the job's | as above |
| a `calendar { }` block's rules — `weekly` / `monthly` / `annual` / `dates` **and** `window` | the **calendar's** | `calendar { timezone }` > `defaults { timezone }` > UTC |

The calendar's zone is deliberately not inherited from whichever job consults
it (issue #450). A calendar is a named, shared resource: "this holiday calendar
is Austrian" has to hold for every job that references it, otherwise the same
calendar would mean a different set of instants per consumer and neither
`GET /v1/calendars` nor the dashboard could say which zone it is in. So a job
in `America/New_York` firing at 22:00 that consults a `Europe/Vienna` calendar
is asking about the *Vienna* day — which, at 22:00 New York time, is already
tomorrow:

```
calendar business-days {
  timezone Europe/Vienna
  include weekly monday tuesday wednesday thursday friday
}

job report:nightly {
  every day at 22:00 { calendar business-days }
  timezone America/New_York   # ← covers the 22:00, not the calendar
}
```

Friday 22:00 in New York is Saturday 04:00 in Vienna, so the gate stays shut
and the job's next fire is the tick whose *Vienna* day is a weekday. Each zone
also follows its own DST switch: the two are three weeks apart in spring, and
during those weeks the same job time lands an hour differently on the
calendar's clock.

Neither zone falls back to the host's `TZ` — one Croniqfile fires at the same
instant in every environment. What croniq does instead is name the zone it
resolved:

- `croniq validate` warns when a wall-clock job (issue #427) **or** a calendar
  with rules (issue #450) has no zone from anywhere: *"its rules are
  interpreted as UTC"*. A warning, never an error — exit stays `0`.
- An unknown IANA name is an error in `validate`/`compile`, a `400` on the
  write that introduced it, and a load fault at the server — never a silent
  fallback (issue #426). The one exception is a `calendar_definitions.timezone`
  row written before that column was validated: it is logged at `WARN` and
  evaluated in UTC rather than pausing every job that consults the calendar.
- The job detail shows the effective job zone next to next-fire, and the
  calendars list shows each calendar's effective zone (`UTC` when unset).

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

## Runner identity ownership

A `runner_id` is bound to the credential that first used it in the work
protocol (first-writer-wins); see the README's *Runner identity in the work
protocol* section for the semantics and the
`pull_api { runner_identity_binding … }` switch. Operationally:

- **Symptom of a mismatch**: work requests return `403 Forbidden` and the
  server logs `work request refused — this runner_id is bound to a different
  credential`, plus a `runner.identity_rejected` audit event (target = the
  runner id). The refusal happens before the registry is touched, so the
  incumbent runner is unaffected: its lease is not extended by the stranger,
  its claims are not requeued, and it is not fenced out with `409`.
- **Runner SDKs treat a `403` as a transient poll error** and keep retrying on
  their poll interval, so a fenced-out runner shows up as one that never
  receives work rather than one that exits. Check the server log or the audit
  trail — the runner side may only log at `debug`.
- **Two causes worth distinguishing.** Either two runners genuinely share a
  `runner_id` and hold different credentials (give each its own id — the same
  advice as for identity flapping above), or a `runner_id` is legitimately
  moving to a new credential (a re-keyed runner pool, a runner migrated to
  another team's client). For the second case, release the binding:

  ```sh
  curl -X DELETE http://localhost:4000/v1/runners/shell-runner-vps-prod \
    -H "Authorization: ApiKey croniq_…"
  ```

  This deregisters the runner *and* frees its id, so the next poll binds it to
  whoever polls first. Requires the `runners:write` scope.
- **Upgrading an existing deployment** needs no preparation: the table starts
  empty, so every runner binds itself on its first poll after the upgrade. The
  window worth knowing about is that first poll — whoever polls first wins the
  id. If an id is bound to the wrong credential, release it as above.
- **A store failure fails closed**: if the binding cannot be read or written,
  work requests get `503` rather than being waved through. Runners retry, so
  this self-heals when the store recovers.

## Orphaned claims (issue #374)

A `claimed` execution whose runner process vanished is recovered by
complementary mechanisms — no operator action needed:

- **Runner restart (same `runner_id`, new `instance_id`)**: the first poll of
  the new process takes the identity over and the old session's claims are
  requeued immediately. Each takeover logs a warning and records a
  `runner.takeover` audit event (target = the runner id). The deposed
  instance id is fenced — if the old process is actually still alive
  (duplicate deployment sharing one `runner_id`), its polls get
  `409 Conflict` and the SDK exits after its conflict streak — three
  consecutive conflicts by default, tunable per runner as
  `max_consecutive_poll_conflicts` (issue #466 brought the budget to the Go,
  Python, TypeScript and Java SDKs; the Rust and .NET ones had it already,
  and the other four retried forever until then).
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
  server restart. Claims with a **fresh lease** are exempt, so a
  slow-but-alive handler is never double-run.
  Each reap logs a `watchdog: requeued stale claimed execution` warning and an
  `execution.stale_claim_requeued` audit event; recurring reaps for the same
  job are the signal to investigate that runner's stability.

  **Leases are per execution** (issue #438). An execution's lease is stamped
  when the work is dispatched and refreshed by either of two things the
  runner does: a poll that lists the execution in its `inflight` array, or a
  `POST /v1/work/renew` naming it. A lease refreshed inside the same
  `max(2 × lease_ttl, 120 s)` window exempts *that* execution and nothing
  else — a runner wedged on some of its claims no longer keeps the reaper off
  all of them because one renew timer is still ticking. The renew endpoint
  enforces the same shape: it verifies the named execution exists, is
  `claimed`, and is held by the calling runner, answering `404` / `409`
  instead of reporting success for an execution it did not consult. Runner
  SDKs run one renew timer per execution, so no runner configuration changes.

  Lease state is in memory: after a server restart it is empty and refills
  from the runners' next polls, well inside the grace window, so a restart
  cannot cause a premature reap.
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

### Per-job liveness series and removed jobs

`croniq_job_last_fire_timestamp`, `croniq_job_next_fire_timestamp` and
`croniq_job_overdue` are emitted only for jobs the running configuration
defines.

Their source, `job_states`, outlives the job that created it — nothing deletes
a row when a job is removed from the Croniqfile. Until
[issue #470](https://github.com/nuetzliches/croniq/issues/470) the exporter read
straight from that table, so a job deleted months earlier kept reporting
`croniq_job_overdue{job_key="demo:smoke"} 1` forever. Anyone following the
recommended `croniq_job_overdue == 1` alert got a permanent false positive they
could only clear with direct SQL against a stopped server.

The rows themselves are still kept — a job commented out for a week should keep
its state, and the loader cannot tell "removed" from "temporarily absent". It
does log them once at startup, naming the keys. To clear one deliberately:

```bash
curl -X DELETE -H "Authorization: ApiKey $ADMIN_KEY"   "$CRONIQ_URL/v1/jobs/demo:smoke"
```

which now removes the `job_states` row along with the definition.

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
executions (`completed` / `failed` / `cancelled`) together with their logs.

A `dead` execution is pruned by these knobs **only when no dead-letter row
references it**. One that has a letter is left to dead-letter retention above,
as before. One that never produced a letter — or whose letter has already been
purged — used to fall through every retention path and grow without bound,
because dead-letter retention only ever deletes from `dead_letters`
([issue #470](https://github.com/nuetzliches/croniq/issues/470)).

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
