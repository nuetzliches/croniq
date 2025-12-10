# Croniq Security Baseline

This document specifies the authentication, authorization, and rate limiting design for Croniq. It extends the guidance captured in `architecture.md` and describes what remains to reach the "Security-Basis" milestone from `CHECKLIST.md`.

## Objectives

- Support API key and OAuth2/OIDC user flows with a shared `ICallerContext` abstraction and tenant-aware partition enforcement.
- Guarantee that every request is rate limited per tenant+caller, while policy-driven quotas inside the Scheduler remain authoritative.
- Keep secrets (API keys, connection strings) outside application binaries by using providers and hashed storage.
- Provide deterministic admin/rotation flows so operators can issue, revoke, and audit identities programmatically.

## Authentication Modes

### API Keys (Machines / Automation)

1. **Provisioning**: Keys are created via `IApiKeyStore.IssueAsync` (backed by the EF-Core provider in `Croniq.Auth.SqlServer`). In-memory mode seeds keys via `Croniq:Auth:InMemory:ApiKey` for samples/tests only.
2. **Persistence**: SQL Server stores only hashed secrets (HMAC SHA-256 + per-key salt). The plaintext is returned once to the operator and never persisted.
3. **Request Flow**: Callers send the key in `X-Croniq-Key`. Middleware resolves the key via `ICallerContextFactory.FromApiKeyAsync`, creating an `ICallerContext` with TenantId, EnvironmentTag, CallerId, and Scopes.
4. **Rotation & Revocation**: Admin APIs (to be exposed under `/tenants/{id}/api-keys`) call `RotateAsync`/`RevokeAsync` and audit the action in `auth.AuditLog`.
5. **Configuration**: `Croniq:Auth:Mode = InMemory|SqlServer`. When `SqlServer`, either reuse `Croniq:SqlServer:ConnectionString` or provide `Croniq:Auth:SqlServer:ConnectionString` explicitly.

### OAuth2 / OIDC (Users)

1. **Support**: `Croniq:Auth:Oidc` options (Authority, Audience, TenantClaim, EnvironmentClaim, ScopeClaims, RequiredScopes, `RequireHttpsMetadata`, etc.) now configure Croniq's built-in OIDC validator. Example:

    ```json
    "Croniq": {
       "Auth": {
          "Mode": "SqlServer",
          "Oidc": {
             "Enabled": true,
             "Authority": "https://login.microsoftonline.com/<tenant>",
             "Audience": "api://cronq",
             "TenantClaim": "tid",
             "EnvironmentClaim": "env",
             "RequiredScopes": [ "cronq.api" ]
          }
       }
    }
    ```

2. **Caller Context**: `ICallerContextFactory.FromBearerTokenAsync` now validates JWTs via the issuer's JWKS metadata, caches the configuration, and maps Croniq-specific fields:
    - Tenant ID resolved from `TenantClaim` (default `tenant`) falling back to `tid` or other configured claims.
    - Environment tag derived from `EnvironmentClaim` with optional fallbacks/defaults.
    - Scopes gathered from `scope`/`scp` style claims, normalized to lowercase when configured; required scopes are enforced before a caller context is emitted.
3. **Route Protection**: The API middleware now inspects `Authorization: Bearer` before falling back to `X-Croniq-Key`. A valid bearer token seeds `ICallerContext` as `CallerType.User`, while API keys remain the default for automation. This keeps both flows consistent without duplicating route attributes yet.
4. **Samples & Docs**: Provide configuration examples for Entra ID and Auth0 in `docs/configuration.md` once the implementation lands.

### Mixed Mode & Future Providers

- Hosts can simultaneously enable API keys and OIDC. The middleware inspects the request headers in order: Bearer token, then API key. Only one caller context is created per request.
- Additional providers (mTLS, external gateway) can plug in through new methods on `ICallerContextFactory` or dedicated middleware.

## Authorization & Tenant Enforcement

- `ICallerContext` lives in scoped DI via `ICallerContextAccessor`. Downstream components (Persistence, JobStore) fetch it to derive `PartitionScope` (TenantId + EnvironmentTag).
- Scope naming convention mirrors REST permissions: `schedules:write`, `jobs:trigger`, `tenants:admin`, `api-keys:manage`, `cluster:read`.
- Admin APIs verify both caller scope and tenant match (e.g., only Tenant Admins can mutate their key space). Cross-tenant actions require service-level credentials flagged with `CallerType = ApiKey` and `Scopes` containing `system:*`.

## Rate Limiting & Quotas

- ASP.NET rate limiting (`AddCroniqApiRateLimiter`) will resolve the caller context first and partition on `TenantId:CallerId`. Anonymous requests use `anonymous`.
- Default policy: fixed window, 60 req/min. Configure overrides via `Croniq:Api:TenantRateLimits:<TenantId>:RequestsPerMinute`.
- Scheduler-level quotas (concurrency, trigger throughput) stay inside `Croniq.Core` using `IPolicyResolver`; HTTP/gRPC rate limiting just protects the ingress.
- gRPC services in `Croniq.Api` register `TenantRateLimitInterceptor`, which acquires the same tenant-aware partitions before any RPC handler runs. Future Croniq gRPC endpoints inherit the HTTP quotas automatically, and retries see `resource-exhausted` with an optional `retry-after` trailer.

Minimal configuration example:

```json
"Croniq": {
   "Api": {
      "RequestsPerMinute": 60,
      "TenantRateLimits": {
         "tenant-a": { "RequestsPerMinute": 200 },
         "tenant-b": { "RequestsPerMinute": 30 }
      }
   }
}
```

## Secrets & Transport

- `ISecretProvider` remains the abstraction for loading API keys/connection strings in hosts; production deployments plug into Vault/KeyVault/Secrets Manager. Local dev may use `.env` or user-secrets.
- All traffic assumes HTTPS. Self-hosted scenarios must trust dev certificates; production requires TLS termination before the API.
- Log and metric hooks redact secrets. Structured logs include TenantId/CallerId only after hashing to prevent leakage.

## Backlog to Reach "Security-Basis"

- [x] Introduce `Croniq:Auth:Oidc` options and wire bearer authentication into `Croniq.Api`.
- [x] Implement `ICallerContextFactory.FromBearerTokenAsync` with tenant/environment/scopes mapping + caching of JWKS metadata.
- [x] Refactor the auth middleware to choose between Bearer and API key flows, and expose `ICallerContext` downstream via features.
- [x] Update rate limiter to partition on `TenantId:CallerId` (fallback to key header when context missing) and expose per-tenant overrides.
- [x] Add gRPC interceptor mirroring the HTTP rate limiter.
- [ ] Create admin endpoints + docs for API key issuance/rotation (ties into `Croniq.Auth.Abstractions` stores).
- [ ] Extend `docs/configuration.md` with an "Authentication" section (API key vs OIDC) and examples.
- [ ] Add automated security regression tests (invalid key, expired key, revoked key, missing scope) under `Croniq.Api.Tests` or the future smoke suite.

Delivering the checklist item means these backlog bullets are implemented and documented, ensuring both API key and OIDC callers share the same enforcement and observability experience.
