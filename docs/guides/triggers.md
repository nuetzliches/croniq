# Croniq Triggers

Triggers decide when a job key should run. Croniq currently supports cron-based schedules. You can create them via the API/gRPC or seed them at worker startup (config or fluent).

## Cron Expressions

Croniq uses 7-field cron expressions (seconds precision). Examples:

```csharp
var everyFiveMinutes = "0 */5 * * * *";
var weekdaysAtNine = "0 0 9 * * MON-FRI";
```

Croniq also supports the special expression `@once` (alias `once`) for a single execution. When used, Croniq schedules exactly one run at `StartAtUtc` if provided, otherwise it fires immediately.

Schedules run in UTC by default. Persisted triggers store the cron expression plus optional start/end bounds.

## Seed Triggers via Configuration

Worker hosts can seed schedules on startup:

```json
{
  "Croniq": {
    "Seeding": { "Mode": "CreateIfMissing" },
    "Triggers": [
      {
        "TriggerId": "samples-smoke-every-5s",
        "JobKey": "samples:smoke",
        "CronExpression": "0/5 * * * * ?",
        "ManagedBy": "Croniq.Sample",
        "Enabled": true
      }
    ]
  }
}
```

### Trigger configuration fields

`Croniq:Triggers` accepts either a JSON array or an object keyed by trigger id. When you use the map form, the key becomes `TriggerId` if the field is omitted.

| Field          | Required | Notes                                                                                                                                          |
| -------------- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| TriggerId      | No       | Defaults to `{JobKey}:{CronExpression}` when omitted.                                                                                          |
| JobKey         | Yes      | Must follow the Croniq job key format (`namespace:name[:variant]`). Tenant/environment are taken from the hosting scope, not from the job key. |
| CronExpression | Yes      | 7-field cron expression, or `@once` for a one-off trigger.                                                                                     |
| StartAtUtc     | No       | Optional UTC start bound (ISO-8601).                                                                                                           |
| EndAtUtc       | No       | Optional UTC end bound (ISO-8601).                                                                                                             |
| Enabled        | No       | Defaults to `true`.                                                                                                                            |
| ManagedBy      | No       | Required when `Croniq:Seeding:Mode=ForceUpdate`; also stored as `metadata.managedBy`.                                                          |
| Metadata       | No       | String dictionary stored with the trigger definition and exposed via `IJobExecutionContext.Metadata`.                                          |

### Metadata conventions

Trigger metadata values are strings. Croniq stores them with the trigger definition and returns them via the schedules API; it does not change scheduling behavior beyond the `managedBy` guard. Pick a predictable encoding for typed values so UI/ops tooling can parse them:

- booleans: `true` or `false`
- numbers: invariant culture (e.g., `12.5`)
- timestamps: ISO-8601 UTC (e.g., `2025-01-01T00:00:00Z`)
- lists: comma-separated values (e.g., `days=MON,WED,FRI`)

Croniq also reserves a few keys during execution: `trigger_id` is injected by the worker, `payload` carries the trigger payload when present, and webhook ingress prefixes metadata with `webhook:*` and `payload:*`. Avoid reusing those names for custom metadata.

## Seed Triggers via Fluent Registration

```csharp
builder.Services
    .AddCroniq()
    .AddCroniqJob("samples", "smoke", (context, _) =>
    {
        context.Logger.LogInformation("Hello from {JobKey}", context.JobKey);
        return Task.CompletedTask;
    })
    .AddTrigger("0/5 * * * * ?", trigger =>
    {
        trigger.ManagedBy = "Croniq.Sample";
    });
```

## Create or Update via API

```bash
curl -X POST https://localhost:5001/tenants/dev-sandbox/schedules \
  "?environment=dev-local" \
  -H "Content-Type: application/json" \
  -H "X-Croniq-Key: <your-dev-key>" \
  -d "{
        \"jobKey\": \"samples:HelloWorld\",
        \"cronExpression\": \"0 * * * * ?\",
        \"enabled\": true
      }"
```

`managedBy` is reserved for config/fluent seeding and is rejected by the schedule API.

## One-off triggers

Use `@once` in schedules or trigger a single run directly:

```csharp
await jobTrigger.TriggerOnceAsync(
  "samples:HelloWorld",
    new Dictionary<string, string> { ["reason"] = "manual" },
    delay: TimeSpan.FromMinutes(5));
```

```bash
curl -X POST https://localhost:5001/jobs/trigger \
  -H "Content-Type: application/json" \
  -H "X-Croniq-Key: <your-dev-key>" \
  -d "{
        \"jobKey\": \"samples:HelloWorld\",
        \"delaySeconds\": 300,
        \"metadata\": { \"reason\": \"manual\" }
      }"
```

### Incoming Webhook Trigger

The `Croniq.Webhooks` host exposes tenant-scoped endpoints such as `POST /webhooks/{hookKey}`. Each hook references a job key and forwards request metadata into the job execution.

```http
POST /webhooks/invoice-paid HTTP/1.1
Host: hooks.croniq.local
X-Croniq-Key: crq_dev_local_sample
X-Croniq-Signature: sha256=...
Content-Type: application/json

{
    "invoiceId": "INV-2024-991",
    "tenant": "eu-shared",
    "amount": 349.0
}
```

The webhook host validates the signature, enforces a per-hook rate limit, then enqueues a trigger with metadata (e.g., `metadata["invoiceId"] = ...`).

Sample configuration (`appsettings.Development.json`) wired up in `Croniq.Sample.ApiHost`:

```json
{
  "Croniq": {
    "Webhooks": {
      "RequestsPerMinute": 30,
      "Endpoints": [
        {
          "HookKey": "invoice-paid",
          "JobKey": "dev:local:samples:smoke",
          "Secret": "dev-webhook-secret",
          "RequireSignature": true,
          "Metadata": {
            "source": "sample",
            "type": "invoice"
          }
        }
      ]
    }
  }
}
```

This exposes `POST /webhooks/invoice-paid` locally; the sample job logs every invocation, and metadata keys such as `payload:invoiceId` become available via `IJobExecutionContext.Metadata`.

Example request against the sample host:

```bash
curl -X POST http://localhost:5199/webhooks/invoice-paid \
  -H "Content-Type: application/json" \
  -H "X-Croniq-Signature: $(python - <<'PY'
import hmac, hashlib, json
secret = b'dev-webhook-secret'
payload = json.dumps({"invoiceId": "INV-2024-991", "amount": 349.0})
sig = hmac.new(secret, payload.encode(), hashlib.sha256).hexdigest()
print(f"sha256={sig}")
PY
)" \
  -d '{"invoiceId":"INV-2024-991","amount":349.0}'
```

`Croniq.Webhooks` recomputes the `sha256=<hex>` signature server-side, applies the per-hook rate limit (default 30 rpm above), and then dispatches the configured job.

#### Webhook Lifecycle

1. **Create or update hooks** via the management API: `POST /tenants/{tenantId}/webhooks?environment=<tag>` with a body such as:

   ```json
   {
     "hookKey": "invoice-paid",
     "jobKey": "dev:local:samples:smoke",
     "secret": "dev-webhook-secret",
     "requestsPerMinute": 30,
     "metadata": { "source": "sample" }
   }
   ```

   DEV stacks can still fall back to `Croniq:Webhooks` config, but production tenants should rely on the API so secrets are persisted in SqlServer.

2. **Fetch capabilities** with `GET /tenants/{tenantId}/webhooks/capabilities?environment=<tag>` to learn the default `requestsPerMinute` and whether unsigned hooks can be enabled before setting `allowUnsigned=true` in the payload.

3. **List existing hooks** with `GET /tenants/{tenantId}/webhooks?environment=<tag>` to verify rate limits, metadata, and enablement before routing callers to the endpoint.
4. **Rotate secrets** with `POST /tenants/{tenantId}/webhooks/{hookKey}/rotate-secret?environment=<tag>`. Provide optional `gracePeriodSeconds` (default 24h) so the previous secret remains valid while upstream systems roll out the new key, and set `activateInSeconds` (up to seven days in the future) when you need a delayed cutover. The rotation response is the only time you see the plaintext secret—stash it in your secret manager immediately or pipe it into your vault automation.

   PowerShell helper (`scripts/webhook-rotate-secret.ps1`) for local workflows:

   ```powershell
   scripts/webhook-rotate-secret.ps1 `
     -TenantId tenant-a `
     -Environment dev `
     -HookKey invoice-paid `
     -ActivateInSeconds 900 `
     -GracePeriodSeconds 86400 `
     -Notes "rotated via devstack"
   ```

   The script prints the activation/expires timestamps and the new secret so you can capture it immediately.

5. **Disable or delete hooks** via `POST` (set `enabled:false`) for temporary pauses or `DELETE /tenants/{tenantId}/webhooks/{hookKey}?environment=<tag>` for permanent removal. Disabled hooks still show up in diagnostics; deleted hooks return `404` immediately.
6. **Audit usage** through telemetry (`Croniq.Webhooks.Ingress` spans) and, once wired up, the `WebhookIngressDeadLetter` table. Until then, structured logs remain the source of truth for per-hook activity.

> Note: Signatures stay mandatory by default. Disable them only when the capabilities endpoint reports `allowUnsignedHooks=true` and you pass `allowUnsigned=true` in the management API payload. In local mode, `Croniq:Webhooks:Security:AllowUnsignedHooks=true` must be set as well. `Croniq.Webhooks` logs a warning the first time an unsigned payload is accepted so you have an audit trail.

### Webhook Security Guidance

Consumers that call Croniq's webhook ingress should implement the following safeguards so every trigger remains tamper-proof and traceable.

#### Signature generation

`X-Croniq-Signature` is `sha256=<hex>` where `<hex>` is the lowercase HMAC-SHA256 digest of the UTF-8 request body using the shared webhook secret. Example implementations:

```csharp
// .NET 8 / C#
using var hmac = new HMACSHA256(Encoding.UTF8.GetBytes(secret));
var payload = JsonSerializer.Serialize(body);
var signature = "sha256=" + Convert.ToHexString(hmac.ComputeHash(Encoding.UTF8.GetBytes(payload))).ToLowerInvariant();
request.Headers.Add("X-Croniq-Signature", signature);
```

```ts
// Node.js / TypeScript
import crypto from "node:crypto";

const payload = JSON.stringify(body);
const digest = crypto
  .createHmac("sha256", secret)
  .update(payload, "utf8")
  .digest("hex");
request.set("X-Croniq-Signature", `sha256=${digest}`);
```

```go
// Go 1.22+
h := hmac.New(sha256.New, []byte(secret))
h.Write(bodyBytes)
signature := fmt.Sprintf("sha256=%x", h.Sum(nil))
req.Header.Set("X-Croniq-Signature", signature)
```

Treat secrets as credentials: read them from your secret manager at runtime, never check them into source control, and rotate them via the management API runbook above.

#### Replay protection

- Send `X-Croniq-Timestamp` with the Unix epoch seconds when the payload was created. Croniq rejects timestamps outside the default ±5-minute window once strict mode is enabled; callers should also refuse to retry a payload once that window has elapsed.
- Add `X-Croniq-Delivery-Id` (a UUID) and store it in your system of record. Croniq logs duplicate IDs, giving you a breadcrumb trail during audits.
- When re-sending after failures, prefer a fresh payload with a new timestamp/id instead of replaying stale data.

#### Recommended headers

- `Content-Type: application/json` (or the actual MIME type) so Croniq enforces payload size/shape.
- `User-Agent` identifying the workload (`sap-billing-forwarder/2.4`).
- `Idempotency-Key` when you orchestrate multiple retries for the same business event; Croniq stores the key inside metadata for downstream jobs.
- `X-Croniq-Tenant` / `X-Croniq-Environment` are **not** required—the hook already maps to a tenant/environment—but you may add informational metadata via `payload:*` fields if you need extra routing context.

#### Backoff and error handling

- `429 ip-rule-denied`: your source IP is not listed. Cross-check the allow list and update it via the API or `WebhookIpRuleClient` before retrying.
- `401 signature-invalid`: regenerate the signature using the newest secret, confirm there is no whitespace/double encoding, and check whether a rotation just occurred (see `WebhookEndpointEvents`).
- `429 rate-limit`: respect the `Retry-After` header. Croniq's fixed window allows short bursts but will throttle noisy callers.

#### Secret rotation

- Subscribe to your CMDB/secret manager so rotations propagate to caller workloads within the grace window returned by `POST .../rotate-secret`.
- After rotating, send health-check payloads using both the old and new secrets to confirm Croniq accepts them until the grace period expires.
- Keep rotation notes (`notes` field) descriptive—Croniq surfaces them in `WebhookEndpointEvents` to speed up incident reviews.

## Enable or Disable Schedules

Update a schedule via the API (or config/seeded triggers) and set `enabled=false` to pause it. Re-enable by setting `enabled=true` or delete the schedule to remove it entirely.

## Configuration Overrides

`Croniq:Triggers` uses the normal configuration pipeline, so environment variables can override JSON values when you need per-environment schedules.
