# Croniq Auth Internals

This document dives deeper into the authentication/authorization subsystem than the consumer `guides/auth.md` page. Use it when implementing providers, admin endpoints, or tenant management tooling.

## Components

| Project                    | Responsibility                                                                                                   |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `Croniq.Auth.Abstractions` | Contracts for `IApiKeyStore`, `ITenantStore`, `ICallerContextFactory`, `ICallerContextAccessor`, scope models.   |
| `Croniq.Auth.Core`         | In-memory store, middleware that inspects headers (`Authorization`, `X-Croniq-Key`), caching of caller contexts. |
| `Croniq.Auth.SqlServer`    | EF Core implementation of `IApiKeyStore` built on `SqlServerDbContext`. Stores hashed secrets, rotation history. |
| `Croniq.Api`               | Wires middleware, rate limiter policies, and admin routes (tenant/key management).                               |

## Authentication Modes

| Mode            | Description                                                                                 | Typical Use                                                         |
| --------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `InMemory`      | Single API key per host (`Croniq:Auth:InMemory:ApiKey`). No persistence.                    | Local dev, unit tests, samples.                                     |
| `SqlServer`     | Multi-tenant API clients/keys persisted in SqlServer. Rotation, revocation, scopes per key. | Production.                                                         |
| `Oidc` (hybrid) | OAuth2/OIDC bearer tokens validated via `Croniq:Auth:Oidc:*` settings.                      | Human operators, portals, automation that already leverages an IdP. |

Croniq inspects headers per request: `Authorization: Bearer ...` first, then `X-Croniq-Key`. Only one `ICallerContext` is produced per request.

## Caller Context Resolution

1. **Bearer token present** → `ICallerContextFactory.FromBearerTokenAsync` validates the token against the configured authority, maps tenant/environment/scopes from claims, builds `CallerContext` with `CallerType = User`.
2. **API key present** → `ICallerContextFactory.FromApiKeyAsync` looks up the key via `IApiKeyStore`, verifies hash/scope, sets `CallerType = ApiKey`.
3. **Neither** → 401 Unauthorized. Rate limiter partitions fallback to `anonymous` scope when context is missing (should be rare outside health endpoints).

### Tenant & Environment Claims

- `Croniq:Auth:Oidc:TenantClaim` defaults to `tenant`, fallback `tid`.
- `Croniq:Auth:Oidc:EnvironmentClaim` defaults to `env`; missing values use the tenant default (configured per host).
- API keys record both tenant and optional environment tag when issued.

## API & Admin Endpoints

Admin routes (scope `tenants:admin`) expose:

- `POST /tenants` – create/update tenant metadata (plan, lifecycle state, default env tag).
- `GET /tenants` / `GET /tenants/{id}` – enumerate tenants and inspect quotas/config.
- `DELETE /tenants/{id}` – deactivate a tenant without removing historical data.
- `POST /tenants/{id}/api-clients` – register a client (name, env tag, default scopes) before issuing keys or tokens.
- `GET /tenants/{id}/api-clients` / `GET /tenants/{id}/api-clients/{clientId}` – list and inspect registered clients.
- `DELETE /tenants/{id}/api-clients/{clientId}` – remove a client and revoke all dependent credentials.
- `POST /tenants/{id}/api-keys` – issue a new key (returns plaintext once).
- `POST /tenants/{id}/api-keys/{keyId}/rotate` – rotate secret, returns new plaintext.
- `DELETE /tenants/{id}/api-keys/{keyId}` – revoke key immediately.
- `GET /tenants/{id}/api-keys` – list active keys with scopes + env tags.
- `POST /tenants/{id}/tokens` – mint a short-lived bearer token that Croniq signiert (Client-Credentials-ähnliche Response mit `accessToken`/`expiresIn`).
- `POST /tenants/{id}/api-clients/{clientId}/tokens` – scoped Variante, wenn mehrere Clients pro Tenant leben (optional `audience`/`scopes`).
- `GET /me` – resolve current caller context (user or API key) for self-checks.

Croniq.Api ships the tenant onboarding, API-client CRUD, token issuance, and `/me` endpoints described above (see [src/Croniq.Api/ApiHostingExtensions.cs](src/Croniq.Api/ApiHostingExtensions.cs)).

### Croniq-issued bearer tokens

- **Motivation**: Operators can bootstrap automation without an external IdP. Croniq's lightweight STS mints JWT access tokens per tenant with the same claims (`tenant`, `env`, `scope`) that the middleware already understands.
- **Flow**: Admins register a tenant + API client and call `POST /tenants/{tenantId}/tokens` (or the client route) with an API key. The response is `{ accessToken, expiresIn, tokenType = "Bearer" }`, mirroring the OAuth2 client-credentials exchange.
- **Signing**: HMAC-SHA256 by default; future releases may add asymmetric keys for public JWKS exposure. The signing key is configured via `Croniq:Auth:Tokens:SigningKey` (Base64) and never leaves the host.
- **Claims**: `sub = clientId`, plus tenant/environment tags from the registered client. `scope` contains the normalized Croniq scopes that were requested (and must be a subset of the client's scopes). Optional `aud` values defend against replay.
- **Lifetimes**: `Croniq:Auth:Tokens:DefaultLifetimeMinutes` controls the default (15 minutes). Callers can override with `ttlMinutes` per request when a shorter validity window is desired.
- **Configuration**: `Croniq:Auth:Tokens:Enabled`, `Issuer`, `DefaultAudience`, `SigningKey`, and `DefaultLifetimeMinutes` all map to `CroniqTokenOptions`. Disable the issuer when an external IdP is mandatory.
- **Admin coverage**: `/me` echoes the resolved caller context (API key vs. Croniq token) so tooling can confirm scopes without parsing JWTs.

### Tenant-/Client-/Token-Endpoints (Design-Spezifikation)

| Method   | Path                                                | Summary                  | Notes                                                                                                                                   |
| -------- | --------------------------------------------------- | ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------- |
| `POST`   | `/tenants`                                          | "Create tenant"          | Body `{ reference, name }`; returns `TenantResponse { tenantId, reference, name, isActive, createdAtUtc }`.                             |
| `GET`    | `/tenants`                                          | "List tenants"           | Optional query `state=active                                                                                                            | all`. Returns collection of `TenantResponse`. |
| `GET`    | `/tenants/{tenantId}`                               | "Get tenant"             | 404 when unknown; response = `TenantResponse`.                                                                                          |
| `DELETE` | `/tenants/{tenantId}`                               | "Deactivate tenant"      | Marks the tenant inactive (soft-delete) and returns `204` or `404` when unknown.                                                        |
| `POST`   | `/tenants/{tenantId}/api-clients`                   | "Register API client"    | Body `{ clientId, name, environmentTag, defaultScopes[] }`; reuses/updates existing row when `clientId` exists.                         |
| `GET`    | `/tenants/{tenantId}/api-clients`                   | "List API clients"       | Optional `environment` filter; returns `ApiClientResponse` list (matches existing `/api-clients/{clientId}` schema).                    |
| `DELETE` | `/tenants/{tenantId}/api-clients/{clientId}`        | "Delete API client"      | Removes the client metadata and revokes any API keys for that client.                                                                   |
| `POST`   | `/tenants/{tenantId}/tokens`                        | "Issue tenant token"     | Body `{ clientId, scopes[], audience, ttlMinutes }`; authenticates via API key or PAT. Returns `{ accessToken, tokenType, expiresIn }`. |
| `POST`   | `/tenants/{tenantId}/api-clients/{clientId}/tokens` | "Issue client token"     | Same payload, automatically infers `clientId` and allowed scopes.                                                                       |
| `GET`    | `/me`                                               | "Inspect caller context" | Echoes resolved tenant/environment/scopes for debugging.                                                                                |

The entries highlighted above are live in the API host today and covered by integration tests ([src/Croniq.Api/ApiHostingExtensions.cs](src/Croniq.Api/ApiHostingExtensions.cs), [tests/Croniq.Api.Tests/TenantAdminEndpointsTests.cs](tests/Croniq.Api.Tests/TenantAdminEndpointsTests.cs)).

#### Request/Response Skizzen

```jsonc
// POST /tenants
{
	"reference": "acme",
	"name": "Acme Corp"
}

// TenantResponse
{
	"tenantId": "tn_d4c1...",
	"reference": "acme",
	"name": "Acme Corp",
	"isActive": true,
	"createdAtUtc": "2025-12-15T11:22:33Z"
}

// POST /tenants/{tenantId}/tokens
{
	"clientId": "portal",
	"scopes": ["jobs:trigger", "schedules:write"],
	"audience": "croniq-api",
	"ttlMinutes": 30
}

// TokenResponse
{
	"accessToken": "eyJhbGciOiJIUzI1NiIs...",
	"tokenType": "Bearer",
	"expiresIn": 1800
}
```

Alle Summaries/Beschreibungen aus der Tabelle landen wortgleich in den neuen OpenAPI-Metadaten, damit Swagger/CLI-Generatoren konsistente Texte zeigen.

## Secret Handling

- API keys follow the format `crq_<segment>_<keyId>_<secret>`; only `keyId` + hashed secret are stored.
- Hashing: HMAC SHA-256 with per-key salt. Secrets never leave memory after issuance.
- Rotation stores audit events (table `auth.AuditLog`, backlog item) for compliance.
- `ISecretProvider` allows binding API keys or connection strings to external secret stores in hosted deployments.

## Rate Limiting & Quotas

- ASP.NET RateLimiter partitions requests by `TenantId:CallerId` based on the resolved caller context.
- Default policy: 60 requests/minute per caller, configured via `Croniq:Api:RequestsPerMinute` with per-tenant overrides (planned config `Croniq:Api:TenantLimits:<tenantId>`).
- Scheduler-level quotas (concurrency, trigger throughput) remain in the policy engine but rely on the same tenant/caller metadata.

## Configuration Reference

| Key                                         | Description                                                                      |
| ------------------------------------------- | -------------------------------------------------------------------------------- |
| `Croniq:Auth:Mode`                          | `InMemory` or `SqlServer`.                                                       |
| `Croniq:Auth:InMemory:ApiKey`               | Plaintext key for in-memory mode.                                                |
| `Croniq:Auth:SqlServer:ConnectionString`    | Optional connection override. Falls back to `Croniq:SqlServer:ConnectionString`. |
| `Croniq:Auth:Oidc:Authority`                | Issuer URL.                                                                      |
| `Croniq:Auth:Oidc:Audience`                 | API resource identifier.                                                         |
| `Croniq:Auth:Oidc:RequiredScopes`           | CSV list enforced at ingress.                                                    |
| `Croniq:Auth:Oidc:TenantClaim`              | Claim name carrying tenant id (default `tenant`).                                |
| `Croniq:Auth:Oidc:EnvironmentClaim`         | Claim name for environment tag (default `env`).                                  |
| `Croniq:Auth:Tokens:Enabled`                | Toggles the built-in Croniq token issuer.                                        |
| `Croniq:Auth:Tokens:Issuer`                 | Value emitted as `iss` for Croniq-minted tokens.                                 |
| `Croniq:Auth:Tokens:DefaultAudience`        | Default `aud` claim when callers omit `audience`.                                |
| `Croniq:Auth:Tokens:SigningKey`             | Base64-encoded symmetric key used for HMAC-SHA256 signing.                       |
| `Croniq:Auth:Tokens:DefaultLifetimeMinutes` | Fallback TTL for minted tokens when `ttlMinutes` is not provided.                |

## Testing & Tooling

- `Croniq.Auth.Core.Tests` verify in-memory stores, middleware flows, and rate limiter partitioning.
- `Croniq.Auth.SqlServer.Tests` use Testcontainers SQL Server to ensure hashing/rotation/audit logic is deterministic.
- Smoke tests in `Croniq.Api.Tests` cover header parsing, mixed-mode auth, and 401/403 paths.

## Backlog

- Add tenant lifecycle hooks (audit events, quotas, default environment metadata) to the existing onboarding routes.
- Emit structured audit logs for key issuance/rotation/revocation and token failures.
- Add caching of JWKS metadata for OIDC providers (per authority) and document cache invalidation knobs.
- Provide automation scripts for issuing keys via CLI (tying into `tools/` folder).
