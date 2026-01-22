# Croniq Authentication Guide

This consumer-focused guide explains how to secure Croniq hosts with API keys and (optionally) bearer tokens. Use it when you deploy `Croniq.Api`, any sample host, or your own app that embeds Croniq services. The deep-dive design remains in `/deep-dive/security.md`.

## Choose an Authentication Mode

| Scenario                                  | Recommended Mode             | Notes                                                                                                                    |
| ----------------------------------------- | ---------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Automation, schedulers, integration tests | API keys                     | Lowest friction, scopes enforced per key, easiest to rotate non-interactive callers.                                     |
| Human operators via UI                    | OIDC login or password login | OIDC login uses your IdP with backend code exchange + HttpOnly refresh cookie; password login uses Croniq-issued tokens. |
| Hybrid workloads                          | Mixed (enable both)          | Croniq inspects `Authorization: Bearer ...` first, then `X-Croniq-Key`. Only one caller context is created per request.  |

You can switch between modes (or enable both) by changing configuration-no code changes are required.

## API Keys

### When to Use API Keys

- Service-to-service calls (CI pipelines, platform schedulers).
- Croniq SDK clients that run outside your identity provider.
- Local development where you do not want to maintain an IdP tenant.

### Provisioning Flow

1. **Pick a backing store**
   - `Croniq:Auth:Mode = InMemory`: single key, best for samples/tests only.
   - `Croniq:Auth:Mode = SqlServer`: production-ready, uses `Croniq.Auth.SqlServer` to hash and store keys.
   - `Croniq:Auth:Mode = Postgres`: production-ready, uses `Croniq.Auth.Postgres` to hash and store keys.

2. **Issue a key**
   - SQL-backed hosts expose admin endpoints under `/tenants/{tenantId}/api-keys`. Call `IApiKeyStore.IssueAsync` only when you need a bootstrap script or console app.
   - In-memory mode reads the secret from `Croniq__Auth__InMemory__ApiKey` (or `appsettings.*`).

3. **Distribute the plaintext** once. Operators copy it into CI/CD variables or `.env.local`. Croniq never stores the plaintext value.
4. **Call the API** with the header `X-Croniq-Key: <your-secret>`.

### API Client IDs

API keys are issued for an **API client** (`ClientId`). The client id is the logical identity used for rate limiting and appears as the `CallerId` in logs. If you run polyglot workers, set `CRONIQ_RUNNER_ID` to the same client id so runner identity and authentication stay aligned.

### Configuration Checklist

| Setting                                                   | Required?             | Description                                                                                |
| --------------------------------------------------------- | --------------------- | ------------------------------------------------------------------------------------------ |
| `Croniq__Auth__Mode`                                      | Always                | `InMemory`, `SqlServer`, or `Postgres`.                                                    |
| `Croniq__Auth__SqlServer__ConnectionString`               | When `SqlServer` mode | Overrides the default Croniq SQL connection if needed.                                     |
| `Croniq__Auth__Postgres__ConnectionString`                | When `Postgres` mode  | Overrides the default Croniq Postgres connection if needed.                                |
| `Croniq__Auth__InMemory__ApiKey`                          | When `InMemory` mode  | Single dev key used by all callers.                                                        |
| `Croniq__Core__TenantId` / `Croniq__Core__EnvironmentTag` | Optional              | Embedded in every issued key so rate limiting and future audit trails remain tenant-aware. |

Example local configuration:

::: code-group

```cmd [Windows cmd]
set Croniq__Auth__Mode=InMemory
set Croniq__Auth__InMemory__ApiKey=crq_dev_local_sample
set Croniq__Core__TenantId=dev-sandbox
set Croniq__Core__EnvironmentTag=dev-jane
```

```dotenv [.env]
CRONIQ_AUTH_MODE=InMemory
CRONIQ_API_KEY=crq_dev_local_sample
CRONIQ_CORE_TENANT_ID=dev-sandbox
CRONIQ_ENVIRONMENT=dev-jane
```

:::

### Rotation & Revocation

- SqlServer/Postgres mode: rotate via `POST /tenants/{tenantId}/api-keys/{keyId}/rotate` and revoke via `DELETE /tenants/{tenantId}/api-keys/{keyId}`. Use `IApiKeyStore.RotateAsync` / `RevokeAsync` only for bootstrap scripts or direct host integrations. Structured audit logging is planned; today, rely on API logs and telemetry for change tracking.
- In-memory mode: update the environment variable and restart the host. Any cached callers must pick up the new value.
- Always remove revoked keys from CI/CD secrets. Croniq rate limiting partitions by Tenant + Caller ID, so stale keys fall back to anonymous throttles and fail fast.

## Password Login

Croniq can expose a username/password login for self-hosted deployments.

- Endpoints: `/auth/login`, `/auth/refresh`, `/auth/logout`, `/auth/change-password`
- Callers must provide `tenantId`.
- Current behavior: the API returns `refreshToken` in the JSON response body and expects it in the request body for refresh/logout.
- Password changes revoke existing refresh tokens; clients must re-login.

See [docs/deep-dive/password-auth.md](../deep-dive/password-auth.md) for details.

## OIDC Login (UI)

Croniq can act as a confidential OIDC client for the admin UI. The browser is redirected to the IdP, the backend exchanges the code, and the UI receives only the access token. Refresh tokens stay in an HttpOnly cookie.

Flow overview:

- `GET /auth/oidc/start` -> redirect to the IdP (PKCE + state handled server-side).
- `GET /auth/oidc/callback` -> backend exchanges the authorization code, sets cookies, and redirects to the UI.
- UI calls `POST /auth/refresh` (with `withCredentials` + `X-CSRF` header from `croniq.oidc.csrf`) to obtain an access token.
- UI calls `POST /auth/logout` with the same CSRF header to clear the refresh cookie.

Configuration checklist (`Croniq:Auth:OidcLogin`):

| Setting                                              | Required? | Description                                         |
| ---------------------------------------------------- | --------- | --------------------------------------------------- |
| `Croniq__Auth__OidcLogin__Enabled`                   | Yes       | Enables the OIDC login flow for the UI.             |
| `Croniq__Auth__OidcLogin__ClientId`                  | Yes       | OIDC client id (confidential client).               |
| `Croniq__Auth__OidcLogin__ClientSecret`              | Yes       | OIDC client secret.                                 |
| `Croniq__Auth__OidcLogin__RedirectUri`               | Yes       | Absolute redirect URI registered with the IdP.      |
| `Croniq__Auth__OidcLogin__UiBaseUrl`                 | Yes       | Base URL of the UI for post-login redirect.         |
| `Croniq__Auth__OidcLogin__Scopes__0`                 | Yes       | Scopes to request (repeat `__Scopes__N` as needed). |
| `Croniq__Auth__OidcLogin__StateTtlMinutes`           | Optional  | PKCE state cookie TTL (minutes).                    |
| `Croniq__Auth__OidcLogin__RefreshCookieLifetimeDays` | Optional  | Refresh cookie lifetime.                            |
| `Croniq__Auth__OidcLogin__CookieSameSite`            | Optional  | Cookie SameSite policy.                             |
| `Croniq__Auth__OidcLogin__CookieDomain`              | Optional  | Cookie domain (omit for host-only).                 |
| `Croniq__Auth__OidcLogin__CookieSecure`              | Optional  | Force Secure flag (defaults to HTTPS).              |

UI runtime config (in `public/assets/croniq-config.json`):

```json
{
  "auth": {
    "mode": "oidc"
  }
}
```

Note: the OIDC login flow issues IdP access tokens. You must enable OIDC bearer validation (`Croniq:Auth:Oidc:Enabled=true`) so the API accepts those tokens.

## OIDC Bearer Tokens (Optional)

Croniq can validate external bearer tokens (for example, Authelia) when `Croniq:Auth:Oidc:Enabled=true`.
When enabled, bearer validation uses the configured OIDC authority only; Croniq-minted bearer tokens
(password login / token issuance) are rejected. API keys remain supported when no `Authorization` header is present.
Tokens must include a tenant claim (default `tenant`, fallback `tid`) or validation fails.

### OIDC Bearer Configuration Checklist

| Setting                                    | Required?               | Description                                                 |
| ------------------------------------------ | ----------------------- | ----------------------------------------------------------- |
| `Croniq__Auth__Oidc__Enabled`              | Yes                     | Enables OIDC/JWT bearer validation.                         |
| `Croniq__Auth__Oidc__Authority`            | Yes                     | OIDC issuer/authority URL.                                  |
| `Croniq__Auth__Oidc__MetadataAddress`      | Optional                | Override discovery metadata address.                        |
| `Croniq__Auth__Oidc__Audience`             | Optional                | Expected `aud` claim (or omit to skip audience validation). |
| `Croniq__Auth__Oidc__RequireHttpsMetadata` | Optional (default true) | Enforce HTTPS for discovery metadata.                       |
| `Croniq__Auth__Oidc__TenantClaim`          | Optional                | Claim name for tenant id (default: `tenant`).               |
| `Croniq__Auth__Oidc__EnvironmentClaim`     | Optional                | Claim name for environment tag (default: `env`).            |
| `Croniq__Auth__Oidc__CallerIdClaim`        | Optional                | Claim name for caller id (default: `sub`).                  |
| `Croniq__Auth__Oidc__ScopeClaims__0`       | Optional                | Scope claim names (defaults include `scope` and `scp`).     |
| `Croniq__Auth__Oidc__RequiredScopes__0`    | Optional                | Scopes required for access (e.g., `ui:access`).             |
| `Croniq__Auth__Oidc__DefaultEnvironment`   | Optional                | Fallback environment tag when not present in claims.        |

Example `.env` snippet:

```dotenv
# Optional OIDC (Authelia example)
CRONIQ_AUTH_OIDC_ENABLED=true
CRONIQ_AUTH_OIDC_AUTHORITY=https://auth.localhost:9091
CRONIQ_AUTH_OIDC_METADATA_ADDRESS=https://auth.localhost:9091/.well-known/openid-configuration
CRONIQ_AUTH_OIDC_AUDIENCE=croniq-api
CRONIQ_AUTH_OIDC_REQUIRED_SCOPES_0=ui:access
```

## Local Development

| Task                      | Tip                                                                                                                                                     |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Use a single shared key   | Keep `Croniq__Auth__Mode=InMemory` for local runs and place the key in `.env`. The AppHost loads `.env` when it starts the dev stack. |
| Simulate multiple tenants | Override `Croniq__Core__TenantId` and `Croniq__Core__EnvironmentTag` per terminal session to see how rate limiting and observability labels change.     |

## FAQs

**How do I know which mode is active?**
Check the startup logs-Croniq logs the effective `Auth.Mode` and the connection string source.

**Can I use mTLS or a gateway instead?**
Yes. Terminate TLS or authenticate upstream (e.g., Azure APIM, Ambassador) and forward either `Authorization` or `X-Croniq-Key` headers. Additional first-class providers will plug into `ICallerContextFactory` later.

**Where do I store secrets?**
Use `ISecretProvider` (Key Vault, AWS Secrets Manager, etc.). The default sample hosts accept `.env.local` for convenience but production deployments should keep secrets outside appsettings.

## Related Docs

- `/introduction/configuration.md` - environment variable matrix and troubleshooting tips.
- `/ops/troubleshooting.md` - common auth failures and rate limit issues.
- `/deep-dive/security.md` - in-depth design, rate limiting, and backlog status.

> **Learn more:** See [security.md](../deep-dive/security.md) and [auth.md](../deep-dive/auth.md) for auth provider contracts, token issuance, and rate-limiting internals.

