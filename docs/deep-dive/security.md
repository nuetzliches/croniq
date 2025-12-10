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
4. **Admin APIs**: Tenant-scoped routes now expose key lifecycle operations behind `api-keys:manage`:

   - `POST /tenants/{tenantId}/api-keys` issues a key for a given client/environment scope and returns the plaintext secret once.
   - `POST /tenants/{tenantId}/api-keys/{keyId}/rotate` deactivates the old secret, issues a replacement, and returns the new secret payload immediately.
   - `DELETE /tenants/{tenantId}/api-keys/{keyId}` revokes a key idempotently.
   - `GET /tenants/{tenantId}/api-clients/{clientId}` surfaces the persisted metadata (scopes, environment tag, activity state) so operators can audit their clients.
     All routes enforce the caller's tenant/environment scoping through `ICallerContext` before touching persistence.

   Example issuance request/response:

   ```http
   POST /tenants/tenant-a/api-keys
   Content-Type: application/json

   {
      "clientId": "deploy-agent",
      "environmentTag": "prod",
      "scopes": [ "jobs:trigger", "schedules:write" ],
      "ttlHours": 48
   }
   ```

   ```json
   {
     "clientId": "deploy-agent",
     "tenantId": "tenant-a",
     "keyId": "ak_123...",
     "plaintextSecret": "ak_123....ZFo=",
     "expiresAtUtc": "2024-05-05T12:00:00Z",
     "environmentTag": "prod"
   }
   ```

   The `plaintextSecret` is never persisted; automation must capture it at issuance/rotation time.

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

## Inbound Webhook Security

- **Dedicated Host**: `Croniq.Webhooks` runs as a standalone ingress surface (or co-hosted inside `Croniq.Api` for dev). It reuses the same DI setup (`Croniq.Hosting`) so persistence/auth/telemetry policies are shared. Operators can place it behind their own gateway/WAF while keeping management APIs separate.
- **Secrets & Signatures**: Every webhook endpoint requires a secret. The host validates `X-Croniq-Signature` using HMAC-SHA256 on the raw request body, then compares hashes in constant time. Secrets live in `croniq.WebhookEndpoints` (SqlServer) with hashes stored alongside metadata; plaintext is only returned when the admin explicitly rotates it via the CRUD API.
- **Rate Limiting**: ASP.NET rate limiter partitions per hook key (falling back to the global default). Limits are configurable through persistence or `Croniq:Webhooks:RequestsPerMinute`. Burst protection keeps individual tenants from exhausting dispatcher capacity.
- **Tenant Scoping**: Persisted hooks include `TenantId` and `EnvironmentTag`. The webhook resolver enforces that the mapped `JobKey` belongs to the same partition before invoking the execution pipeline, preventing cross-tenant spoofing even when secrets leak.
- **Metadata Sanitization**: Incoming JSON payloads are stored as metadata with `payload:*` prefixes. The factory performs best-effort extraction (strings/numbers/bools) and never stores raw headers. Operators can disable metadata enrichment per hook at configuration time if data minimization is required.
- **Secret Rotation Flow**: `POST /tenants/{tenantId}/webhooks` accepts updated secrets and returns them once. Automation can perform staged rotations by creating a temporary hook with the same job key or by coordinating clients to pick up the new secret immediately. Every rotation writes to `croniq.WebhookSecretHistory`, so the previous secret stays valid until the configured grace window expires; the ingress host resolves all active secrets via `GetActiveSecretsAsync` and validates payloads against each value.
- **Recommended Hardening**: Front webhook hosts with IP allow lists or API Gateway auth, configure OWASP rules for JSON bodies, and send telemetry (`Croniq.Webhooks.Ingress`) to your SIEM for anomaly detection (e.g., spikes in 401/429).

### Per-Hook IP Allow Lists

- **Schema & rollout**: The `WebhookEndpointIpRules` table stores CIDR blocks per hook/tenant/environment. Apply the `20251212104500_AddWebhookEndpointIpRules` migration via `Croniq.DbMigrator` before enabling the feature (runbook in `docs/deep-dive/persistence.md`). The schema addition is backward compatible, so existing hooks stay open until rules are created.
- **Ingress enforcement**: `Croniq.Webhooks` compiles the stored CIDRs into `IpNetwork` instances during endpoint hydration. Requests are rejected with `403 ip-rule-denied` when the remote address falls outside every configured network. Empty rule sets keep endpoints open, letting operators stage the rollout hook-by-hook.
- **Admin APIs**: Tenant-scoped management APIs expose CRUD operations guarded by `webhooks:read`/`webhooks:write`:

  - `GET /tenants/{tenantId}/webhooks/{hookKey}/ip-rules?environment=prod`
  - `POST /tenants/{tenantId}/webhooks/{hookKey}/ip-rules?environment=prod`
  - `DELETE /tenants/{tenantId}/webhooks/{hookKey}/ip-rules/{ruleId}?environment=prod`

  `POST` validates CIDR syntax server-side and normalizes the stored network. Duplicate CIDRs return `409 conflict`. Responses echo audit metadata (`CreatedBy`, timestamps) so operators can compare the deployed allow list with their CMDB.

  ```http
  POST /tenants/tenant-a/webhooks/hook_123/ip-rules?environment=prod
  Content-Type: application/json

  {
      "cidr": "203.0.113.0/28",
      "description": "Core banking outbound"
  }
  ```

- **Operations**:
  1.  Decide the default posture (open vs closed). For closed-by-default, seed a catch-all rule (`0.0.0.0/0`) before tightening to explicit CIDRs.
  2.  Script creation/deletion via the API (or upcoming SDK helper) to keep Croniq in sync with your source-of-truth IP inventory.
  3.  Monitor `Croniq.Webhooks.Ingress` metrics/logs for the `ip-rule-denied` counter; alert when the rate exceeds baseline to catch accidental lockouts.

### Broader Ingress Tests & Visibility

- **E2E coverage**: `tests/Croniq.Api.Smoke` now runs both the management scenario (`Webhook_ip_rule_crud_roundtrip`) and the ingress exercise (`Webhook_ingress_respects_ip_rules`), which hits `Croniq.Webhooks` to assert `403 ip-blocked` followed by `202 accepted` after adding a catch-all rule.
- **UI/SDK surfacing**: Expose list/create/delete IP rule helpers inside the Operator UI and `Croniq.Sdk` so operators do not handcraft HTTP calls. This also centralizes validation/error translations.
- **Telemetry dashboards**: Extend the observability pack to chart `ip-rule-denied`, `signature-invalid`, and rate-limit signals together. Distinguishing between denied IPs and failed signatures shortens incident triage.

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
- [x] Create admin endpoints + docs for API key issuance/rotation (ties into `Croniq.Auth.Abstractions` stores).
- [x] Extend `docs/configuration.md` with an "Authentication" section (API key vs OIDC) and examples.
- [x] Add automated security regression tests (invalid key, expired key, revoked key, missing scope) under `Croniq.Api.Tests` or the future smoke suite.
- [x] Harden webhook ingress: per-hook IP allow lists and guardrails for payload size/content-type.
- [ ] Build webhook allow-list smoke tests (allow + deny paths) and payload size guardrail coverage under `tests/Croniq.Api.Smoke`.
- [ ] Expose allow-list CRUD in the Operator UI / `Croniq.Sdk` to replace ad-hoc HTTP calls.
- [ ] Add webhook-specific security docs for consumers (`guides/triggers.md`) covering signature generation, replay protection, and recommended HTTP headers.

Delivering the checklist item means these backlog bullets are implemented and documented, ensuring both API key and OIDC callers share the same enforcement and observability experience.
