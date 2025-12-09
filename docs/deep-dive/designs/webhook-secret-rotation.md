# Webhook Secret Rotation & Signature Opt-Out

## Goals

- Support zero-downtime secret rotation for persisted webhooks by storing previous and upcoming secrets with bounded grace windows.
- Provide operational insight into rotations (who rotated, when activated, when retired).
- Keep signature validation mandatory by default while exposing an explicit, audited opt-out switch for trusted environments.

## Data Model

- New table `croniq.WebhookSecretHistory` with columns:
  - `Id` (identity)
  - `HookKey`, `TenantId`, `EnvironmentTag`
  - `Secret` (encrypted at rest once KMS support lands; plaintext stored temporarily for compatibility)
  - `SecretHash` (HMAC SHA-256 of secret)
  - `ActivatedAtUtc` (defaults to now)
  - `ExpiresAtUtc` (nullable; null means active)
  - `RotatedBy` (string up to 128 chars)
  - `Notes` (nullable)
- Unique filtered index `(HookKey, TenantId, EnvironmentTag)` with `ExpiresAtUtc IS NULL OR ExpiresAtUtc > sysutcdatetime()` to guarantee max two active rows (current + grace).
- EF entity + migration `20251210103000_AddWebhookSecretHistory` to create table + indexes.

## Persistence Flow

- `SqlServerWebhookPersistenceProvider` gains methods:
  - `RotateSecretAsync(WebhookSecretRotate request)` (new abstraction on `IWebhookPersistenceProvider`).
  - Internal helpers to append history row + update `WebhookEndpointEntity.Secret`/`SecretHash`.
- `WebhookEndpointEntity` continues to hold current secret for backward compatibility but setter only triggered via rotation path.
- When rotating:
  1. Validate optional `activateIn` delay + `gracePeriod` (default immediate + 24h grace).
  2. Insert new history row with future `ActivatedAtUtc` if delay specified.
  3. Update previous active row's `ExpiresAtUtc` to `ActivatedAtUtc + gracePeriod`.
  4. Return plaintext secret to caller exactly once.
- Signature validation queries `WebhookSecretHistory` for active rows and accepts any whose window contains `UtcNow`.
- Maintenance job (future) can purge expired history based on retention.

## API Surface

- New DTO `WebhookSecretRotateRequest { int? ActivateInSeconds, int? GracePeriodSeconds, string? Notes }`.
- Endpoint: `POST /tenants/{tenantId}/webhooks/{hookKey}/rotate-secret` (scope via `?environment=<tag>` query parameter).
- Response returns `HookKey`, `ActivatedAtUtc`, `ExpiresAtUtc`, `Secret`, `SecretHash`.

### Calling the rotation endpoint

```bash
curl -s -X POST "https://api.croniq.dev/tenants/{tenantId}/webhooks/{hookKey}/rotate-secret?environment=dev" \
  -H "Content-Type: application/json" \
  -H "X-Croniq-Key: <admin-api-key>" \
  -d '{
        "activateInSeconds": null,
        "gracePeriodSeconds": 3600,
        "notes": "rotated via deploy pipeline"
      }'
```

The API returns the plaintext secret exactly once. Persist it in your secret manager immediately; Croniq only stores the hash + metadata. `RotatedBy` derives from the authenticated caller (`ICallerContextAccessor`) so CI/service principals show up as `apiKey:{clientId}`.

You can wrap the call inside a script (PowerShell, Bash, etc.) to mask the new secret before printing it or to push it downstream (Azure Key Vault, AWS Secrets Manager). A future helper script will live under `scripts/webhook-rotate-secret.ps1`, but until then the raw HTTP call above is the reference flow.

## Signature Opt-Out Controls

- Config flag `Croniq:Webhooks:Security:AllowUnsignedHooks` (bool, default `false`). `CroniqWebhookOptions.Security.AllowUnsignedHooks` exposes the same toggle for code-first hosts.
- Admin API rejects `RequireSignature = false` unless the flag is `true` **and** callers pass `?allowUnsigned=true` on the request (safety net against accidental opt-out).
- For config-defined endpoints, `AddCroniqWebhookServices` throws during startup when an unsigned hook is encountered without the flag.
- Ingress bypasses validation only when both the descriptor opts out and the flag is enabled. The first unsigned request emits a warning (`webhook {HookKey} accepts unsigned payloads because AllowUnsignedHooks=true`) so operators have an audit trail.

## Open Questions

- Key management: secrets remain plaintext for now; once the secrets provider supports encryption, `WebhookSecretHistory.Secret` becomes encrypted blob and responses fetch decrypted value on-demand.
- Audit fields (`RotatedBy`) sourced from authenticated principal in Admin API; CLI can pass custom value (e.g., `"cli:devops"`).
- Retention policy: default 90 days for expired rows; configurable via `Croniq:Webhooks:SecretHistoryRetentionDays` (future work).
