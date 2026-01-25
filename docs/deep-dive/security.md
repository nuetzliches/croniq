# Croniq Security Baseline

This document specifies the authentication, authorization, and rate limiting design for Croniq. It extends the guidance captured in `architecture.md` and describes what remains to reach the "Security-Basis" milestone tracked in `BACKLOG.md`.

::: info Status
Implemented (baseline). Last verified: 2026-01-18.
:::

## Objectives

- Support API key and user bearer-token flows with a shared `ICallerContext` abstraction and tenant-aware partition enforcement.
- Guarantee that every request is rate limited per tenant+caller, while policy-driven quotas inside the Scheduler remain authoritative.
- Keep secrets (API keys, connection strings) outside application binaries by using providers and hashed storage.
- Provide deterministic admin/rotation flows so operators can issue, revoke, and audit identities programmatically.

## Authentication Modes

### API Keys (Machines / Automation)

1. **Provisioning**: Keys are created via `IApiKeyStore.IssueAsync` (backed by the EF-Core providers in `Croniq.Auth.SqlServer` or `Croniq.Auth.Postgres`). In-memory mode seeds keys via `Croniq:Auth:InMemory:ApiKey` for samples/tests only.
2. **Persistence**: SqlServer/Postgres stores only hashed secrets (HMAC SHA-256 + per-key salt). The plaintext is returned once to the operator and never persisted.
3. **Request Flow**: Callers send the key in `X-Croniq-Key`. Middleware resolves the key via `ICallerContextFactory.FromApiKeyAsync`, creating an `ICallerContext` with TenantId, EnvironmentTag, CallerId (API client id), and Scopes.
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

5. **Configuration**: `Croniq:Auth:Mode = InMemory|SqlServer|Postgres`. When `SqlServer`, either reuse `Croniq:SqlServer:ConnectionString` or provide `Croniq:Auth:SqlServer:ConnectionString` explicitly; when `Postgres`, use `Croniq:Postgres:ConnectionString` or `Croniq:Auth:Postgres:ConnectionString`.

### Bearer Tokens (Users)

1. **Support**: Croniq can validate bearer tokens and map tenant/environment/scopes from claims.
   Forward-looking federated-login details are intentionally out of scope for these public docs.

2. **Caller Context**: `ICallerContextFactory.FromBearerTokenAsync` validates JWTs via issuer metadata, caches configuration, and maps Croniq-specific fields:
   - Tenant ID resolved from `TenantClaim` (default `tenant`) or another explicitly configured claim.
   - Environment tag derived from `EnvironmentClaim` (optional).
   - Scopes gathered from `scope`/`scp` style claims, normalized to lowercase when configured; required scopes are enforced before a caller context is emitted.
3. **Route Protection**: The API middleware now inspects `Authorization: Bearer` before falling back to `X-Croniq-Key`. A valid bearer token seeds `ICallerContext` as `CallerType.User`, while API keys remain the default for automation. This keeps both flows consistent without duplicating route attributes yet.
4. **Samples & Docs**: Provide configuration examples for Entra ID and Auth0 in `docs/configuration.md` once the implementation lands.

### Mixed Mode & Future Providers

- Hosts can simultaneously enable API keys and bearer tokens. The middleware inspects the request headers in order: Bearer token, then API key. Only one caller context is created per request.
- Additional providers (mTLS, external gateway) can plug in through new methods on `ICallerContextFactory` or dedicated middleware.

## Authorization & Tenant Enforcement

- `ICallerContext` lives in scoped DI via `ICallerContextAccessor`. Downstream components (Persistence, JobStore) fetch it to derive `PartitionScope` (TenantId + EnvironmentTag).
- `TenantGuard` enforces caller tenant/environment across REST routes (webhooks CRUD, schedules, manual triggers) and rejects cross-tenant attempts with 403 before persistence/pipeline execution. The execution-log endpoint now inspects the first log entry to validate tenant/environment metadata before streaming; Scheduler/Worker/Webhook ingress gRPC hosts already apply the same guard.
- Scope naming convention mirrors REST permissions: `schedules:write`, `jobs:trigger`, `tenants:admin`, `api-keys:manage`.
- Bearer tokens must carry the configured tenant claim and any required scopes; missing claims/scopes yield 401/403. API keys remain single-tenant because validation bakes the tenant into the emitted caller context.
- gRPC Scheduler: the Scheduler service runs in `Croniq.Api` (mapped via `MapCroniqSchedulerGrpc`). Calls use the same middleware/guards as HTTP; clients must send `x-croniq-key` (or Bearer) metadata and align `tenant_id`/`environment_tag` (required on `DeleteSchedule`). The proto lives under `src/Croniq.Rpc.Client/Protos/scheduler.proto`; usage is documented in the gRPC guide and client samples. Safe wrappers emit `CroniqRpcException` to avoid direct coupling to `Grpc.Core`.
- Admin APIs verify both caller scope and tenant match (e.g., only Tenant Admins can mutate their key space). Cross-tenant actions require service-level credentials flagged with `CallerType = ApiKey` and `Scopes` containing `system:*`.

## Inbound Webhook Security

- **Dedicated Host**: `Croniq.Webhooks` runs as a standalone ingress surface (or co-hosted inside `Croniq.Api` for dev). It reuses the same DI setup (`Croniq.Hosting`) so persistence/auth/telemetry policies are shared. Operators can place it behind their own gateway/WAF while keeping management APIs separate.
- **Secrets & Signatures**: Every webhook endpoint requires a secret. The host validates `X-Croniq-Signature` using HMAC-SHA256 on the raw request body, then compares hashes in constant time. Persisted webhook secrets are encrypted at rest via ASP.NET Core Data Protection and stored alongside a SHA-256 hash; plaintext is only returned when the admin explicitly creates or rotates the secret via the CRUD API.
- **Rate Limiting**: ASP.NET rate limiter partitions per hook key (falling back to the global default). Limits are configurable through persistence or `Croniq:Webhooks:RequestsPerMinute`. Burst protection keeps individual tenants from exhausting dispatcher capacity.
- **Tenant Scoping**: Persisted hooks include `TenantId` and `EnvironmentTag`. Admin and ingress paths enforce the tenant/environment scope, and dispatch uses the same partition when invoking the execution pipeline. Ingress still requires the job to be registered on the host (otherwise it returns `job-not-registered`).
- **Metadata Sanitization**: Incoming JSON payloads are stored as metadata with `payload:*` prefixes. The factory performs best-effort extraction (strings/numbers/bools) and never stores raw headers. Metadata enrichment is always enabled; keep payloads minimal if data minimization is required.
- **Secret Rotation Flow**: `POST /tenants/{tenantId}/webhooks` accepts updated secrets and returns them once. Automation can perform staged rotations by creating a temporary hook with the same job key or by coordinating clients to pick up the new secret immediately. Every rotation writes to `croniq.WebhookSecretHistory`, so the previous secret stays valid until the configured grace window expires; the ingress host resolves all active secrets via `GetActiveSecretsAsync` and validates payloads against each value.
- **Recommended Hardening**: Front webhook hosts with IP allow lists or API Gateway auth, configure OWASP rules for JSON bodies, and send telemetry (`Croniq.Webhooks.Ingress`) to your SIEM for anomaly detection (e.g., spikes in 401/429).

### Per-Hook IP Allow Lists

- **Schema & rollout**: The `WebhookEndpointIpRules` table stores CIDR blocks per hook/tenant/environment. Apply the EF Core migration via `Croniq.DbMigrator` before enabling the feature (runbook in `docs/deep-dive/persistence.md`). The schema addition is backward compatible, so existing hooks stay open until rules are created.
- **Ingress enforcement**: `Croniq.Webhooks` compiles the stored CIDRs into `IpNetwork` instances during endpoint hydration. Requests are rejected with `403 ip-blocked` when the remote address falls outside every configured network. Empty rule sets keep endpoints open, letting operators stage the rollout hook-by-hook.
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

1. Decide the default posture (open vs closed). For closed-by-default, seed a catch-all rule (`0.0.0.0/0`) before tightening to explicit CIDRs.
2. Script creation/deletion via the API or the `Croniq.Sdk.Operator.Webhooks.WebhookIpRuleClient` helper to keep Croniq in sync with your source-of-truth IP inventory.
3. Monitor `Croniq.Webhooks.Ingress` metrics/logs for the `ip-blocked` counter; alert when the rate exceeds baseline to catch accidental lockouts.
4. Every CRUD/rotation call writes to `WebhookEndpointEvents`; the API publishes cache-invalidating notifications so the ingress host refreshes CIDRs within seconds (no manual restarts required).
5. Include `X-Croniq-CorrelationId` on IP rule management requests so `WebhookEndpointEvents` can stitch a trail today; extending `Actor`/`CorrelationId` capture to the rest of the webhook CRUD surface is still backlog.

### Broader Ingress Tests & Visibility

- **E2E coverage**: `tests/Croniq.Api.Smoke` now runs both the management scenario (`Webhook_ip_rule_crud_roundtrip`) and the ingress exercise (`Webhook_ingress_respects_ip_rules`), which hits `Croniq.Webhooks` to assert `403 ip-blocked` followed by `202 accepted` after adding a catch-all rule.
- **Telemetry dashboards**: Extend the observability pack to chart `ip-blocked`, `signature-invalid`, and rate-limit signals together. Distinguishing between denied IPs and failed signatures shortens incident triage.

### Operator UI & SDK Roadmap

- **Surface CRUD affordances**: The Operator UI should list the current CIDRs next to each webhook, gate edits behind `webhooks:write`, and reuse the API validation messages so operators get immediate syntax feedback. Provide bulk add/remove to mirror common firewall workflows.
- **Tenant-safe defaults**: Default new hooks to an explicit posture toggle (open vs closed) and display warning banners when an allow list is empty in a production environment. SDK helpers should expose `EnsureIpRulesAsync` convenience methods that idempotently converge the desired CIDR set.
- **Audit context**: Both UI and SDK calls must stamp `ChangedBy` metadata with the authenticated operator/service identity and persist correlation IDs so `WebhookEndpointEvents` can be tied back to UI actions during incident reviews (`X-Croniq-CorrelationId` header).
- **Automation hooks**: Document how to script `cronq sdk webhooks ip-rules sync --tenant tenant-a --hook hook_123` once the CLI lands, ensuring GitOps flows can drive the same APIs without custom glue.

#### Croniq.Sdk helper (available)

`Croniq.Sdk.Operator.Webhooks.WebhookIpRuleClient` now wraps the `/webhooks/{hookKey}/ip-rules` endpoints, handling JSON payloads, CIDR normalization, correlation headers, and `ProblemDetails` errors via `CroniqApiException`. Example usage:

```csharp
var httpClient = new HttpClient
{
   BaseAddress = new Uri("https://cronq.local"),
};
httpClient.DefaultRequestHeaders.Add("X-Croniq-Key", apiKey);

var ipRules = new WebhookIpRuleClient(httpClient);
var syncResult = await ipRules.SyncAsync(
   tenantId: "tenant-a",
   hookKey: "hook_123",
   environment: "prod",
   desiredRules: new[]
   {
      new WebhookIpRuleDesired("203.0.113.0/28", "Branch router"),
      new WebhookIpRuleDesired("198.51.100.0/24", "Fraud cluster"),
   },
   correlationId: "change-req-9241");

Console.WriteLine($"Created {syncResult.Created.Count} rules, deleted {syncResult.DeletedRuleIds.Count}.");
```

`SyncAsync` always creates new CIDRs before removing old networks, so an allow list can shift without a gap in coverage. UI flows can call the same helper (passing through their own correlation IDs), or invoke `CreateAsync`/`DeleteAsync` for granular control.

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
- Webhook secret encryption relies on ASP.NET Core Data Protection; API and webhook hosts must share the same key ring (`Croniq:Security:DataProtection:KeyRingPath`) and application name (`Croniq:Security:DataProtection:ApplicationName`, defaults to `Croniq`) to decrypt persisted secrets.
  - Remote Compose (API + webhooks): set `Croniq__Security__DataProtection__KeyRingPath=/var/lib/croniq/keys` and (optionally) `Croniq__Security__DataProtection__ApplicationName=Croniq` on both containers and mount the same volume to `/var/lib/croniq/keys`.
  - The production compose template wires this up via `CRONIQ_SECURITY_DATAPROTECTION_KEYRINGPATH` + `CRONIQ_SECURITY_DATAPROTECTION_APPLICATIONNAME` and a shared `croniq-dp-keys` volume.
  - Rotate webhook secrets after changing the key ring path or application name.
- All traffic assumes HTTPS. Self-hosted scenarios must trust dev certificates; production requires TLS termination before the API.
- Log and metric hooks redact secrets. TenantId/CallerId are hashed when `Croniq:Observability:HashIdentifiers=true` with `Croniq:Observability:IdentifierHashKey`; otherwise they remain clear text.

## Operational Runbooks & Incident Response

- **API key compromise**: (1) call `DELETE /tenants/{tenantId}/api-keys/{keyId}` to revoke the offender, (2) rotate dependent deploy agents via `POST .../rotate`, (3) search `Croniq.Api.Auth` logs for the compromised CallerId to confirm no lingering traffic, and (4) invalidate cached rate-limiter entries by restarting only the edge pod if requests keep flowing (the limiter refreshes partitions automatically once traffic stops).
- **Webhook secret/IP rule drift**: When secrets or CIDRs must change quickly, batch rotations through the existing CRUD APIs - each call emits a `WebhookEndpointEvents` record and pushes cache invalidations to every ingress replica. No manual recycle is required; confirm the rollout by tailing `Croniq.Webhooks` logs for `cache invalidation completed` and running the smoke tests.
- **Database least privilege**: Deploy `Croniq.DbMigrator` with a migration role, then run the API/Worker/Webhook hosts under read/write roles scoped to their schemas (`croniq`, `auth`). Deny `ALTER` to runtime identities and audit failed DDL attempts via SQL Server Extended Events or Postgres audit logs.
- **Telemetry to SIEM**: Forward `Croniq.Api` and `Croniq.Webhooks` structured logs plus `Croniq.Observability` metrics to your SIEM. Prioritize alerts for `auth.failed`, `rate.limit.rejected`, `ip-blocked`, and unusual spikes in `WebhookDeadLetters`.
- **Disaster recovery validation**: Quarterly, restore the database backups into a staging cluster, run `Croniq.DbMigrator` to verify schema parity, then execute the Aspire smoke flow (`tools/Croniq.Devstack.AppHost` + `tests/Croniq.Api.Smoke`) against the restored environment to ensure creds, secrets, and webhook caches hydrate as expected.

## Webhook Consumer Guidance

- Detailed guidance now lives in `docs/guides/webhooks.md` under **Webhook Security Guidance**. It covers signature generation samples (C#, Node.js, Go), recommended headers, and ingress error handling.
- Error-handling expectations (`ip-blocked`, `signature-invalid`, `rate-limit`) plus rotation best practices reference the `WebhookEndpointEvents` audit stream so operators can confirm which secret is active.

## Backlog to Reach "Security-Basis"

- [x] Wire bearer authentication into `Croniq.Api`.
- [x] Implement `ICallerContextFactory.FromBearerTokenAsync` with tenant/environment/scopes mapping + caching of JWKS metadata.
- [x] Refactor the auth middleware to choose between Bearer and API key flows, and expose `ICallerContext` downstream via features.
- [x] Update rate limiter to partition on `TenantId:CallerId` (fallback to key header when context missing) and expose per-tenant overrides.
- [x] Add gRPC interceptor mirroring the HTTP rate limiter.
- [x] Create admin endpoints + docs for API key issuance/rotation (ties into `Croniq.Auth.Abstractions` stores).
- [x] Extend configuration docs with an "Authentication" section and examples.
- [x] Add automated security regression tests (invalid key, expired key, revoked key, missing scope) under `Croniq.Api.Tests` or the future smoke suite.
- [x] Harden webhook ingress: per-hook IP allow lists.
- [ ] Add payload size/content-type guardrails for webhook ingress.
- [x] Build webhook allow-list smoke tests (allow + deny paths) under `tests/Croniq.Api.Smoke` (`Webhook_ip_rule_crud_roundtrip`, `Webhook_ingress_respects_ip_rules`).
- [ ] Add payload size/content-type guardrail coverage under `tests/Croniq.Api.Smoke` once the guardrails ship.
- [ ] Expose allow-list CRUD in the Operator UI / `Croniq.Sdk` to replace ad-hoc HTTP calls.
  - [ ] (deferred until ui is ready) Ship a webhook details panel that lists CIDRs, supports inline add/remove, and blocks edits without `webhooks:write` scope.
  - [x] Add idempotent helpers to `Croniq.Sdk` + CLI (`cronq webhooks ip-rules sync`) so automation converges desired rule sets (`WebhookIpRuleClient.SyncAsync`).
  - [x] Record `ChangedBy` + correlation IDs from both surfaces (stored in `WebhookEndpointEvents` + SDK correlation header support).
  - [ ] Add end-to-end UI automation (Playwright/integration harness) once the Operator UI ships to exercise the SDK-powered workflow.
- [x] Add webhook-specific security docs for consumers (`docs/guides/webhooks.md`) covering signature generation and recommended HTTP headers.
  - [x] Publish step-by-step signature verification samples (Go/Node/.NET) that match `X-Croniq-Signature` semantics.
  - [x] Document required signature headers (`X-Croniq-Signature`) and backoff expectations for 429s.
  - [x] Include rotation guidance referencing `WebhookEndpointEvents`, plus troubleshooting flow for `signature-invalid` metrics.

Delivering the checklist item means these backlog bullets are implemented and documented, ensuring both API key and bearer-token callers share the same enforcement and observability experience.
