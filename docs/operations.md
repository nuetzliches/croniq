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
  "version":    "0.4.2",                  // from CARGO_PKG_VERSION
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

### Probing from the UI: `GET /v1/auth/config`

```jsonc
GET /v1/auth/config  →  200
{
  "oidc": {
    "enabled": true,
    "provider_name": "Authentik",
    "login_url": "http://localhost:4000/v1/auth/oidc/login"
  },
  "password": { "enabled": false }
}
```

The login UI hits this endpoint before any authentication happens and
uses the response to:

* hide the password form when `password.enabled === false`
* hide the SSO card when `oidc.enabled === false`
* show a "no sign-in method configured" blocker if both are off
  (mostly a defence-in-depth — the server refuses to boot in that
  state, so this branch is mainly for misconfigured load-balancers
  that hide one endpoint from the client)

The older `GET /v1/auth/oidc/config` continues to return its original
flat shape (`{enabled, provider_name, login_url}`) and is unaffected
by `auth.password.enabled` — keep using it if you have an external
probe that only cares about the SSO half.
