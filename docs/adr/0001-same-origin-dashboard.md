# ADR-0001: The dashboard is served from the same origin as the API

- **Status:** Accepted
- **Date:** 2026-08 (recorded 2026-09-09)
- **Related:** [#454](https://github.com/nuetzliches/croniq/issues/454) (refresh token out of `localStorage`), [#429](https://github.com/nuetzliches/croniq/issues/429) (CORS + security headers), [#431](https://github.com/nuetzliches/croniq/issues/431) (session invalidation)

## Context

A password or SSO login mints two credentials with very different lifetimes: an
access token (stateless JWT, one hour) and a refresh token (opaque, seven days,
stored hashed server-side).

Until #454 the dashboard kept both in `localStorage`. Any XSS — or a
compromised npm dependency executing at runtime — could lift the refresh token
and hold the account for a week. That survives reloads and outlives the access
token, which `token_generation` otherwise makes cheap to revoke. The dashboard
pulls a three-digit number of transitive npm packages, so "a dependency
executes hostile code once" is not a hypothetical threat model.

A refresh token can be put out of JavaScript's reach only in an `HttpOnly`
cookie, and a cookie is bound to an origin.

## Decision

The dashboard is served from the same origin as the API. The refresh token
lives in a `croniq_refresh` cookie — `HttpOnly; SameSite=Strict;
Path=/v1/auth` — and the access token lives in memory only, never persisted.
Every API call other than `POST /v1/auth/refresh` authenticates with an
`Authorization: Bearer` header, so no ambient authority exists anywhere else.

A cross-origin dashboard remains buildable, but only as an acknowledged trade:
`VITE_API_URL` without `VITE_ALLOW_LOCALSTORAGE_REFRESH=1` fails the build with
an explanation rather than silently producing a `localStorage` bundle.

## Alternatives considered

- **Keep both tokens in `localStorage`, shorten the refresh lifetime.** Reduces
  the window, does not remove the exposure, and shortening it enough to matter
  turns every idle tab into a re-login.
- **`SameSite=None; Secure` + `Access-Control-Allow-Credentials`.** Would let
  the dashboard live anywhere. It also reintroduces a CSRF surface on
  `/v1/auth/refresh` that `SameSite=Strict` removes for free, and forces the
  CORS layer to trust an origin list at runtime. Rejected: the flexibility buys
  a deployment topology nobody had asked for, at the cost of a class of bug.
- **Refresh-token rotation with reuse detection instead of an `HttpOnly`
  cookie.** Detects theft after the fact; the attacker still gets one full
  session. Worth having *in addition*, not instead.
- **No refresh token at all — re-login every hour.** Rejected on operator
  experience; a scheduler dashboard is left open for hours.

## Consequences

- **A reload starts with no access token.** The dashboard silently calls
  `POST /v1/auth/refresh`, which the browser answers with the cookie. This is
  why a reload shows a brief spinner rather than the login page.
- **Logging out is a server round-trip.** Clearing a cookie does not revoke the
  token behind it, so `POST /v1/auth/logout` revokes server-side and clears the
  cookie in one response.
- **Serving the dashboard from a separate container requires a reverse proxy in
  front** that routes `/` and `/v1` to one origin. Without it, the split is a
  deliberate security regression, not a deployment detail. See
  [ADR-0002](0002-single-image-delivery.md).
- **`Secure` appears only when the server can tell the page is on HTTPS**
  (`Origin`, `X-Forwarded-Proto`, or an `https://` `app_url`). Browsers never
  return a `Secure` cookie over plain HTTP, so setting it unconditionally would
  lock out plain-HTTP deployments rather than harden them.
- **Non-browser clients are unaffected.** The CLI, curl and the SDKs keep
  receiving `refresh_token` in the login response body. Cookie delivery is
  opt-in per request (`"refresh_cookie": true`).
- An XSS can still act as the user while the page is open. The CSP is what
  limits that blast radius, which is why it is part of this constraint rather
  than an unrelated hardening measure.

## Enforced by

- `crates/croniq-server/src/api/refresh_cookie.rs` — cookie name, attributes, delivery mode
- `crates/croniq-server/src/api/auth_endpoints.rs` — login/refresh/logout paths that choose body vs. cookie
- `crates/croniq-server/src/api/hardening.rs` — `CONTENT_SECURITY_POLICY` (`connect-src 'self'`), origin-locked CORS, no `Allow-Credentials`
- `ui/vite.config.ts` — `assertTokenStorageAcknowledged`, the build guard
- `ui/src/api/base.ts` — same-origin default (`""` base URL)
- `docs/operations.md` → "Where the dashboard keeps its tokens"

## What this ADR does not say

It does not say the dashboard must be served *by `croniq-server`*. Same origin
is the constraint; a reverse proxy that unifies two containers under one origin
satisfies it. It also does not forbid a cross-origin build — it makes one an
explicit, acknowledged choice.
