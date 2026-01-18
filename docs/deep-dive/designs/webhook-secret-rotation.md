# Webhook Secret Rotation & Signature Opt-Out

::: info Status
Implemented. Last verified: 2026-01-18.
:::

## Goals

- Support zero-downtime secret rotation for persisted webhooks by storing previous and upcoming secrets with bounded grace windows.
- Provide operational insight into rotations (who rotated, when activated, when retired).
- Keep signature validation mandatory by default while exposing an explicit, audited opt-out switch for trusted environments.

## Data Model

- New table `croniq.WebhookSecretHistory` with columns:
  - `Id` (identity)
  - `HookKey`, `TenantId`, `EnvironmentTag`
  - `Secret` (encrypted at rest via ASP.NET Core Data Protection)
  - `SecretHash` (HMAC SHA-256 of secret)
  - `ActivatedAtUtc` (defaults to now)
  - `ExpiresAtUtc` (nullable; null means active)
  - `RotatedBy` (string up to 128 chars)
  - `Notes` (nullable)
- Unique filtered index `(HookKey, TenantId, EnvironmentTag)` with `ExpiresAtUtc IS NULL OR ExpiresAtUtc > sysutcdatetime()` to guarantee max two active rows (current + grace).
- EF entity + migration to create the table + indexes.

## Persistence Flow

- `SqlServerWebhookPersistenceProvider` and `PostgresWebhookPersistenceProvider` gain methods:
  - `RotateSecretAsync(WebhookSecretRotate request)` (new abstraction on `IWebhookPersistenceProvider`).
  - Internal helpers to append history row + update `WebhookEndpointEntity.Secret`/`SecretHash`.
- `WebhookEndpointEntity` continues to hold current secret for backward compatibility but setter only triggered via rotation path.
- When rotating:
  1. Validate optional `activateIn` delay + `gracePeriod` (default immediate + 24h grace). Activation delays are capped at seven days to avoid indefinite future secrets.
  2. Insert new history row with future `ActivatedAtUtc` if delay specified.
  3. Update the current active row's `ExpiresAtUtc` to `ActivatedAtUtc + gracePeriod` and trim any other pending rows so only the primary + grace window remain active at a time.
  4. Return plaintext secret to caller exactly once.
- Signature validation queries `WebhookSecretHistory` for active rows and accepts any whose window contains `UtcNow`.
- Retention cleanup uses `Croniq:Retention` settings (`WebhookSecretHistoryEnabled`, `WebhookSecretHistoryExpiryOffsetDays`) when the retention job is enabled; default expiry offset is 7 days after `ExpiresAtUtc`.

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

The API returns the plaintext secret exactly once. Persist it in your secret manager immediately; Croniq stores the encrypted secret plus hash metadata. `RotatedBy` derives from the authenticated caller (`ICallerContextAccessor`) so CI/service principals show up as `apiKey:{clientId}`.

You can wrap the call inside a script (PowerShell, Bash, etc.) to mask the new secret before printing it or to push it downstream (Azure Key Vault, AWS Secrets Manager). The `scripts/webhook-rotate-secret.ps1` helper automates the request and is a good reference for CI/local flows.

### Helper script

`scripts/webhook-rotate-secret.ps1` automates the HTTP call for local operators and CI pipelines. Parameters:

- `-TenantId`, `-Environment`, `-HookKey`: scope and webhook identifier.
- Optional `-ActivateInSeconds` (0-604800) to stage a future activation window.
- Optional `-GracePeriodSeconds` (defaults to server-side 24h) to keep the previous secret alive while callers switch.
- Optional `-Notes` to capture runbooks or ticket IDs in the audit trail.

Example:

```powershell
scripts/webhook-rotate-secret.ps1 `
  -TenantId tenant-a `
  -Environment dev `
  -HookKey deploy-trigger `
  -ActivateInSeconds 1800 `
  -GracePeriodSeconds 86400 `
  -Notes "rotated before blue/green cutover"
```

The script prints the activation/grace window plus the plaintext secret, making it straightforward to pipe into vault tooling or clipboard managers.

## Signature Opt-Out Controls

- Config flag `Croniq:Webhooks:Security:AllowUnsignedHooks` (bool, default `false`). `CroniqWebhookOptions.Security.AllowUnsignedHooks` exposes the same toggle for code-first hosts.
- Admin API rejects `RequireSignature = false` unless the flag is `true` **and** callers pass `allowUnsigned=true` in the request payload (safety net against accidental opt-out).
- For config-defined endpoints, `AddCroniqWebhookServices` throws during startup when an unsigned hook is encountered without the flag.
- Ingress bypasses validation only when both the descriptor opts out and the flag is enabled. The first unsigned request emits a warning (`webhook {HookKey} accepts unsigned payloads because AllowUnsignedHooks=true`) so operators have an audit trail.

## Open Questions

- Key management: secrets are encrypted at rest via Data Protection; decide whether to centralize the key ring (shared volume, key store) or integrate an external KMS as a follow-up.
- Audit fields (`RotatedBy`) sourced from authenticated principal in Admin API; CLI can pass custom value (e.g., `"cli:devops"`).
- Retention policy uses the Croniq retention job (`Croniq:Retention:WebhookSecretHistoryEnabled`, `Croniq:Retention:WebhookSecretHistoryExpiryOffsetDays`); defaults keep history for 7 days after expiry once retention is enabled.
