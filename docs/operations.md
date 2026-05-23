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
