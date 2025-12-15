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

- `POST /tenants` – create/update tenant metadata (plan, lifecycle state, default env tag).
- `GET /tenants` / `GET /tenants/{id}` – enumerate tenants and inspect quotas/config.
- `POST /tenants/{id}/api-clients` – register a client (name, env tag, default scopes) before issuing keys or tokens.
- `GET /tenants/{id}/api-clients` / `GET /tenants/{id}/api-clients/{clientId}` – list and inspect registered clients.
- `POST /tenants/{id}/api-keys` – issue a new key (returns plaintext once).
- `POST /tenants/{id}/api-keys/{keyId}/rotate` – rotate secret, returns new plaintext.
- `DELETE /tenants/{id}/api-keys/{keyId}` – revoke key immediately.
- `GET /tenants/{id}/api-keys` – list active keys with scopes + env tags.
- `POST /tenants/{id}/tokens` – mint a short-lived bearer token that Croniq signiert (Client-Credentials-ähnliche Response mit `accessToken`/`expiresIn`).
- `POST /tenants/{id}/api-clients/{clientId}/tokens` – scoped Variante, wenn mehrere Clients pro Tenant leben (optional `audience`/`scopes`).
- `GET /me` – resolve current caller context (user or API key) for self-checks.

Implementation status is tracked in `security.md` backlog; this document will reflect the routes once they ship. The API-key CRUD routes already exist; tenant management + token issuance remain on the roadmap (see CHECKLIST entry "Croniq-internes Token-Issuing").

### Croniq-issued bearer tokens (planned)

- **Motivation**: Operators want to bootstrap automations without wiring an external IdP. Croniq therefore ships eine leichte STS, die JWT-Access-Tokens pro Tenant signiert. Diese Tokens tragen die gleichen Claims (`tenant`, `env`, `scopes`) wie externe OIDC-Tokens, sodass Middleware unverändert bleiben kann.
- **Flow**: Admin registriert Tenant + API-Client und ruft danach `POST /tenants/{tenantId}/tokens` (oder die Client-Variante) mit API-Key/Credentials auf. Die Antwort liefert `{ accessToken, expiresIn, tokenType = "Bearer" }` analog zum OAuth2 Client-Credentials-Flow.
- **Signing**: Standardmäßig HMAC-SHA256; perspektivisch Signaturzertifikate (siehe Supply-Chain-Hardening). Public Keys landen unter `/.well-known/openid-configuration` + JWKS, damit Hosts Tokens offline validieren können.
- **Claims**: `sub = clientId`, `tenant` + `env` stammen vom Client, `scope` enthält normalisierte Croniq-Scopes, optional `aud` gegen Replay.
- **Lifetimes**: Default 15 Minuten, overrides pro Tenant/Client. Refresh Tokens zunächst out-of-scope; Caller fordern neue Tokens via API-Key oder Client-Secret an.
- **Admin coverage**: `GET /me` liefert den aktuellen Caller (API-Key vs. Croniq-Token), damit Tooling Scopes prüfen kann ohne JWT zu decodieren. Zukünftige Admin-UIs verwenden dieselben Routen.

### Tenant-/Client-/Token-Endpoints (Design-Spezifikation)

| Method | Path | Summary | Notes |
| ------ | ---- | ------- | ----- |
| `POST` | `/tenants` | "Create tenant" | Body `{ reference, name }`; returns `TenantResponse { tenantId, reference, name, isActive, createdAtUtc }`.
| `GET` | `/tenants` | "List tenants" | Optional query `state=active|all`. Returns collection of `TenantResponse`.
| `GET` | `/tenants/{tenantId}` | "Get tenant" | 404 when unknown; response = `TenantResponse`.
| `POST` | `/tenants/{tenantId}/api-clients` | "Register API client" | Body `{ clientId, name, environmentTag, defaultScopes[] }`; reuses/updates existing row when `clientId` exists.
| `GET` | `/tenants/{tenantId}/api-clients` | "List API clients" | Optional `environment` filter; returns `ApiClientResponse` list (matches existing `/api-clients/{clientId}` schema).
| `POST` | `/tenants/{tenantId}/tokens` | "Issue tenant token" | Body `{ clientId, scopes[], audience, ttlMinutes }`; authenticates via API key or PAT. Returns `{ accessToken, tokenType, expiresIn }`.
| `POST` | `/tenants/{tenantId}/api-clients/{clientId}/tokens` | "Issue client token" | Same payload, automatically infers `clientId` and allowed scopes.
| `GET` | `/me` | "Inspect caller context" | Echoes resolved tenant/environment/scopes for debugging.

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
