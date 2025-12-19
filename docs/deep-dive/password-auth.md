# Password Authentication (Username/Password) – Concept

This document defines a first-party username/password login concept for Croniq.

It complements (not replaces) the existing authentication modes documented in [docs/deep-dive/auth.md](./auth.md). Password auth is primarily aimed at self-hosted environments without an external identity provider.

## Goals

- Provide a **concrete login endpoint** for human operators that yields a bearer token usable with existing Croniq APIs.
- Keep the system **secure by default** against brute force, credential stuffing, session theft, replay, and database compromise.
- Keep the system **scalable** (stateless access tokens; server-side refresh token store; horizontal scaling).
- Preserve tenant isolation: users are bound to tenants and scopes; isolation is enforced end-to-end.
- Keep dependencies compatible with the repo policy (MIT/Apache/BSD).

## V1 Scope (Self-hosted Single Tenant)

The initial implementation target is a **self-hosted deployment inside a private network** with a focus on **one tenant**.

- Tenant selection should be **optional** for end users.
- The server resolves the tenant via a configured default.
- Federated/IdP-backed human authentication is explicitly deferred to a later phase.

## Non-Goals

- Implementing OAuth2 "resource owner password" grant (ROPC) for third-party apps.
- Replacing external identity providers.
- Building a full IAM product (SSO federation, SCIM, etc.).

## Key Design Decisions

### 1) Transport Security is non-negotiable

- All auth endpoints require HTTPS.
- If Croniq is deployed behind a reverse proxy, enforce `ForwardedHeaders` + HTTPS redirection and document the required proxy config.

### 2) Client-side hashing: not a security substitute

A plain "hash password in the UI and send the hash" approach is **not** equivalent to not sending the password:

- The hash becomes a **password-equivalent secret**: if leaked (logs, browser extension, XSS, proxy), it can be replayed.
- It does not protect against credential stuffing; it can worsen it by creating a stable credential.
- It prevents server-side upgrades (hash parameters, peppering strategy) because the server no longer sees the password.

**Therefore**: Croniq should not implement “hash-in-UI and send hash” as the primary mechanism.

If you truly need "never send the password" semantics, use a PAKE protocol.

### 3) If “no raw password over the wire” is required: use PAKE (OPAQUE/SRP)

- **Recommended**: OPAQUE (modern PAKE) or SRP as a fallback.
- This requires a vetted, well-maintained library with acceptable licensing.
- Without such a library, the safer approach is: **TLS + server-side password hashing + strong anti-abuse controls**.

Decision point:

- **Option A (recommended MVP)**: Standard login over HTTPS, server verifies password.
- **Option B (advanced)**: PAKE (OPAQUE preferred) to avoid transmitting the password.

The rest of this document describes Option A as the baseline, and outlines Option B as an extension.

## Option A (Baseline): Standard Login + Access/Refresh Tokens

### Endpoints (HTTP)

- `POST /auth/login`

  - Body: `{ tenantId?, tenantReference?, username, password, environmentTag?, audience?, scopes? }`
  - In V1, `tenantId`/`tenantReference` are typically omitted.
  - Returns: `{ tenantId, accessToken, tokenType: "Bearer", expiresIn, refreshToken }`

- `POST /auth/refresh`

  - Rotates refresh token.
  - Body: `{ tenantId?, refreshToken, environmentTag?, audience?, scopes? }`
  - Returns: `{ accessToken, tokenType: "Bearer", expiresIn, refreshToken }`

- `POST /auth/logout`

  - Revokes refresh token (server-side).
  - Body: `{ tenantId?, refreshToken }`

- `POST /auth/change-password`

  - Changes the password for the currently authenticated password user.
  - Requires a valid access token.
  - Body: `{ currentPassword, newPassword }`
  - After a successful password change, all refresh tokens for the user are revoked. Clients must re-login to obtain a new refresh token.

- Optional admin endpoints (restricted):
  - `POST /tenants/{tenantId}/users` create user / invite.
  - `POST /tenants/{tenantId}/users/{userId}/reset-password`.

### Token Model

- **Access token**: JWT, short-lived (e.g. 5–15 minutes).

  - Claims: `sub` (user id), `tenant`, `env` (optional), `scope`, `amr=pwd`, `sid` (session id), `jti`.
  - Signed with the existing token signing infrastructure (`Croniq:Auth:Tokens:*`) or a dedicated key.

- **Refresh token**: opaque random value.
  - Stored server-side (SQL) as a hash (`SHA-256` or stronger) + metadata.
  - Rotation on every refresh; reuse detection → revoke the entire session.

### Password Storage (Server)

- Store passwords using a slow adaptive KDF.
  - If using ASP.NET Core Identity: the built-in password hasher (PBKDF2 by default) is acceptable.
  - If implementing a custom hasher: prefer Argon2id (needs a vetted library + license check).
- Add a **pepper** stored in the secret provider / environment (never in the DB).
- Enforce password policy: length, banned passwords, and reset flow.

### Abuse & Hardening

- Rate limiting:
  - Per tenant + IP + username for `/auth/login`.
  - Global circuit breaker for spikes.
- Lockout:
  - Temporary lockout after N failed attempts with exponential backoff.
- Credential stuffing protections:
  - Optional CAPTCHA/turnstile integration (deferred; keep hooks).
- Audit logging:
  - Successful logins, failures (with reason), refresh, logout.
  - Do not log passwords or tokens.

### UI Storage Guidance

- Prefer a **BFF pattern**: UI keeps only a session cookie (HttpOnly). API calls happen through the BFF.
- If the UI must call APIs directly:
  - Store access tokens only in memory (not localStorage).
  - Prefer a refresh token cookie for browser-only clients; for non-browser clients, body transport is often simpler.

## Refresh token transport (concrete trade-offs)

Croniq can transport refresh tokens in two common ways.

**Stand jetzt**: refresh tokens are returned in the JSON response body and must be provided in the request body for `/auth/refresh` and `/auth/logout`.

### Variant A: refresh token in JSON body (current)

#### Pros (Variant A)

- Works uniformly for browsers, CLIs, and service-to-service clients.
- No ambient-cookie CSRF surface.
- Easier debugging (explicit payload).

#### Cons (Variant A)

- In browsers, you must store the refresh token somewhere: any XSS can typically exfiltrate it.
- Higher risk of accidental logging/telemetry leakage (payloads, headers, SDK traces).

### Variant B: refresh token as HttpOnly Secure cookie (typical for UI/BFF)

#### Pros (Variant B)

- Better XSS resilience for refresh tokens (JS cannot read HttpOnly cookies).
- Natural fit for a BFF: browser never handles a long-lived secret.

#### Cons (Variant B)

- Requires CSRF strategy (at minimum: `SameSite` and/or CSRF token depending on deployment).
- More ops complexity (domain/path/secure/samesite, reverse proxy behavior).
- Less convenient for non-browser clients unless you also provide a body-based flow.

## Option B (Advanced): PAKE (OPAQUE/SRP)

### High-level Flow

1. `POST /auth/pake/start` → server returns a challenge.
2. Client computes response using username/password and the challenge.
3. `POST /auth/pake/finish` → server verifies without learning the password.
4. Issue the same access/refresh tokens as Option A.

### Notes

- This is significantly more complex to implement and test.
- Requires careful cryptographic review, library selection, and protocol parameter hardening.
- The main benefit is “password not transmitted”, but it does **not** replace HTTPS.

## Data Model (SQL)

Minimal tables (names illustrative):

- `auth.Users`

  - `UserId` (pk), `TenantId`, `UsernameNormalized`, `PasswordHash`, `IsActive`, timestamps.

- `auth.RefreshTokens`
  - `TokenId` (pk), `UserId`, `TenantId`, `TokenHash`, `CreatedAtUtc`, `ExpiresAtUtc`, `RevokedAtUtc`, `ReplacedByTokenId`, `Ip`, `UserAgent`.

## Configuration (V1)

### Default tenant

To make tenant selection optional for end users, configure a default tenant reference:

- `Croniq:Auth:Password:DefaultTenant`
  - Must be a **tenant reference** (not an id).
  - Used when `/auth/login`, `/auth/refresh`, or `/auth/logout` omit tenant information.

Note: Password auth user records are tenant-scoped; without a default tenant the server cannot resolve a tenant from `username` alone.

### Environment tag semantics

`environmentTag` is treated as a **partition/preset** within a tenant.

- It is stored as a claim in the access token.
- Refresh tokens are not bound to an environment.
- Clients may request a different `environmentTag` on refresh to switch environments without re-entering the password.

## Tenant & environment: "Stand jetzt" and open decisions

### Stand jetzt (V1)

- Tenant can be omitted for password endpoints if `Croniq:Auth:Password:DefaultTenant` is configured.
- `environmentTag` is treated as a partition/preset and is carried in the access token.
- Refresh can switch environments by passing a different `environmentTag`.

### Open decisions

- Bind refresh tokens to environments (force re-login on env switch) vs allow env switch via refresh.
- Require explicit tenant selection even in single-tenant installs vs rely on default tenant.
- Add dedicated audit events when environment switches happen via refresh.

## Tenants & Scopes

- Users are tenant-scoped.
- A user has assigned scopes; requested scopes must be a subset.
- The `CallerContextFactory` should map password-auth tokens into `CallerType = User` with consistent claims.

## Configuration

Proposed configuration keys:

- `Croniq:Auth:Password:Enabled` (default `false`)
- `Croniq:Auth:Password:Lockout:*` (thresholds/durations)
- `Croniq:Auth:Password:TokenLifetimeMinutes`
- `Croniq:Auth:Password:RefreshLifetimeDays`

## Rollout Plan

1. Add this concept doc and align it with `docs/deep-dive/security.md`.
2. Decide Option A vs. Option B (PAKE) based on licensing and complexity.
3. Implement Option A behind `Croniq:Auth:Password:Enabled`.
4. Add integration tests (rate limiting, lockout, refresh rotation, tenant isolation).
