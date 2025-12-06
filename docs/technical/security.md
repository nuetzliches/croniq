# Croniq Security Baseline

This document specifies the authentication, authorization, and rate limiting design for Croniq. It extends the guidance in `CONCEPT.md` (sections 3a, 9, 14, 18) and describes what remains to reach the "Security-Basis" milestone from `CHECKLIST.md`.

## Objectives

- Support API key and OAuth2/OIDC user flows with a shared `ICallerContext` abstraction and tenant-aware partition enforcement.
- Guarantee that every request is rate limited per tenant+caller, while policy-driven quotas inside the Scheduler remain authoritative.
- Keep secrets (API keys, connection strings) outside application binaries by using providers and hashed storage.
- Provide deterministic admin/rotation flows so operators can issue, revoke, and audit identities programmatically.

## Authentication Modes

### API Keys (Machines / Automation)

1. **Provisioning**: Keys are created via `IApiKeyStore.IssueAsync` (backed by Xtraq procs such as `[auth].[ApiKeyIssue]`). In-memory mode seeds keys via `Croniq:Auth:InMemory:ApiKey` for samples/tests only.
2. **Persistence**: Xtraq stores only hashed secrets (HMAC SHA-256 + per-key salt). The plaintext is returned once to the operator and never persisted.
3. **Request Flow**: Callers send the key in `X-Croniq-Key`. Middleware resolves the key via `ICallerContextFactory.FromApiKeyAsync`, creating an `ICallerContext` with TenantId, EnvironmentTag, CallerId, and Scopes.
4. **Rotation & Revocation**: Admin APIs (to be exposed under `/tenants/{id}/api-keys`) call `RotateAsync`/`RevokeAsync` and audit the action in `auth.AuditLog`.
5. **Configuration**: `Croniq:Auth:Mode = InMemory|Xtraq`. When `Xtraq`, either reuse `Croniq:Xtraq:ConnectionString` or provide `Croniq:Auth:Xtraq:ConnectionString` explicitly.

### OAuth2 / OIDC (Users)

1. **Support**: Add `Croniq:Auth:Oidc` options (Authority, Audience, RequiredScopes, TenantClaim). The API host enables `JwtBearerDefaults.AuthenticationScheme` and `AddAuthorization()`.
2. **Caller Context**: Implement `ICallerContextFactory.FromBearerTokenAsync` to parse JWTs, validate signatures with the authority JWKS, and map tenant/environment scopes:
   - Tenant ID resolved from `tenant`, `tid`, or a custom claim configured via options.
   - Environment tag derived from `env` claim or default per tenant.
   - Scopes come from the `scope`/`scp` claim; missing required scopes reject the request.
3. **Route Protection**: Minimal API endpoints specify `[Authorize(AuthenticationSchemes = JwtBearerDefaults.AuthenticationScheme)]` for user flows while keeping API-key auth as the default. A dual-auth middleware chooses caller context based on the presence of `Authorization: Bearer` vs `X-Croniq-Key`.
4. **Samples & Docs**: Provide configuration examples for Entra ID and Auth0 in `docs/consumer/configuration.md` once the implementation lands.

### Mixed Mode & Future Providers

- Hosts can simultaneously enable API keys and OIDC. The middleware inspects the request headers in order: Bearer token, then API key. Only one caller context is created per request.
- Additional providers (mTLS, external gateway) can plug in through new methods on `ICallerContextFactory` or dedicated middleware.

## Authorization & Tenant Enforcement

- `ICallerContext` lives in scoped DI via `ICallerContextAccessor`. Downstream components (Persistence, JobStore) fetch it to derive `PartitionScope` (TenantId + EnvironmentTag).
- Scope naming convention mirrors REST permissions: `schedules:write`, `jobs:trigger`, `tenants:admin`, `api-keys:manage`, `cluster:read`.
- Admin APIs verify both caller scope and tenant match (e.g., only Tenant Admins can mutate their key space). Cross-tenant actions require service-level credentials flagged with `CallerType = ApiKey` and `Scopes` containing `system:*`.

## Rate Limiting & Quotas

- ASP.NET rate limiting (`AddCroniqApiRateLimiter`) will resolve the caller context first and partition on `TenantId:CallerId`. Anonymous requests use `anonymous`.
- Default policy: fixed window, 60 req/min. Options allow tenants to override via `Croniq:Api:RateLimits:<TenantId>` or environment tag filters.
- Scheduler-level quotas (concurrency, trigger throughput) stay inside `Croniq.Core` using `IPolicyResolver`; HTTP/gRPC rate limiting just protects the ingress.
- gRPC clients receive the same guard via an interceptor that shares the limiter partition storage (e.g., `PartitionedRateLimiter.Create` with a distributed store if required later).

## Secrets & Transport

- `ISecretProvider` remains the abstraction for loading API keys/connection strings in hosts; production deployments plug into Vault/KeyVault/Secrets Manager. Local dev may use `.env` or user-secrets.
- All traffic assumes HTTPS. Self-hosted scenarios must trust dev certificates; production requires TLS termination before the API.
- Log and metric hooks redact secrets. Structured logs include TenantId/CallerId only after hashing to prevent leakage.

## Backlog to Reach "Security-Basis"

- [ ] Introduce `Croniq:Auth:Oidc` options and wire `JwtBearer` authentication into `Croniq.Api`.
- [ ] Implement `ICallerContextFactory.FromBearerTokenAsync` with tenant/environment/scopes mapping + caching of JWKS metadata.
- [ ] Refactor the auth middleware to choose between Bearer and API key flows, and expose `ICallerContext` downstream via features.
- [ ] Update rate limiter to partition on `TenantId:CallerId` (fallback to key header when context missing) and expose per-tenant overrides.
- [ ] Add gRPC interceptor mirroring the HTTP rate limiter.
- [ ] Create admin endpoints + docs for API key issuance/rotation (ties into `Croniq.Auth.Abstractions` stores).
- [ ] Extend `docs/consumer/configuration.md` with an "Authentication" section (API key vs OIDC) and examples.
- [ ] Add automated security regression tests (invalid key, expired key, revoked key, missing scope) under `Croniq.Api.Tests` or the future smoke suite.

Delivering the checklist item means these backlog bullets are implemented and documented, ensuring both API key and OIDC callers share the same enforcement and observability experience.
