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

Admin routes (scope `tenants:admin`) will expose:

- `POST /tenants` – create tenant metadata (plan, default env tag).
- `POST /tenants/{id}/api-keys` – issue a new key (returns plaintext once).
- `POST /tenants/{id}/api-keys/{keyId}/rotate` – rotate secret, returns new plaintext.
- `DELETE /tenants/{id}/api-keys/{keyId}` – revoke key immediately.
- `GET /tenants/{id}/api-keys` – list active keys with scopes + env tags.
- `GET /me` – resolve current caller context (user or API key) for self-checks.

Implementation status is tracked in `security.md` backlog; this document will reflect the routes once they ship.

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

| Key                                      | Description                                                                      |
| ---------------------------------------- | -------------------------------------------------------------------------------- |
| `Croniq:Auth:Mode`                       | `InMemory` or `SqlServer`.                                                       |
| `Croniq:Auth:InMemory:ApiKey`            | Plaintext key for in-memory mode.                                                |
| `Croniq:Auth:SqlServer:ConnectionString` | Optional connection override. Falls back to `Croniq:SqlServer:ConnectionString`. |
| `Croniq:Auth:Oidc:Authority`             | Issuer URL.                                                                      |
| `Croniq:Auth:Oidc:Audience`              | API resource identifier.                                                         |
| `Croniq:Auth:Oidc:RequiredScopes`        | CSV list enforced at ingress.                                                    |
| `Croniq:Auth:Oidc:TenantClaim`           | Claim name carrying tenant id (default `tenant`).                                |
| `Croniq:Auth:Oidc:EnvironmentClaim`      | Claim name for environment tag (default `env`).                                  |

## Testing & Tooling

- `Croniq.Auth.Core.Tests` verify in-memory stores, middleware flows, and rate limiter partitioning.
- `Croniq.Auth.SqlServer.Tests` use Testcontainers SQL Server to ensure hashing/rotation/audit logic is deterministic.
- Smoke tests in `Croniq.Api.Tests` cover header parsing, mixed-mode auth, and 401/403 paths.

## Backlog

- Implement and document the admin routes listed above.
- Emit structured audit logs for key issuance/rotation/revocation and token failures.
- Add caching of JWKS metadata for OIDC providers (per authority) and document cache invalidation knobs.
- Provide automation scripts for issuing keys via CLI (tying into `tools/` folder).
