# Croniq Triggers

Triggers decide when a job key should run. Configure them per job via the fluent builder or centrally through configuration.

## Cron Expression

```csharp
job.WithCronTrigger("0 */5 * * * *"); // every 5 minutes
```

Uses standard CRON format (seconds precision). Combine with `TimeZoneInfo` if you need region-specific execution:

```csharp
job.WithCronTrigger("0 0 9 * * MON-FRI", tz => tz.WithTimeZone("Europe/Berlin"));
```

## Fixed Interval

```csharp
job.WithIntervalTrigger(TimeSpan.FromSeconds(30));
```

Simple heartbeat jobs; Croniq prevents overlapping runs if the handler is still active.

## Calendar Window

```csharp
job.WithCalendarTrigger(calendar =>
{
    calendar.Weekdays(DayOfWeek.Monday, DayOfWeek.Wednesday);
    calendar.At(14, 30);
});
```

Useful for business schedules without maintaining full CRON strings.

## Event-Driven / Ad-hoc

```csharp
job.WithEventTrigger(trigger =>
{
    trigger.Source = "orders";
    trigger.Match(metadata => metadata.ContainsKey("priority"));
});
```

Push notifications or webhooks can enqueue context payloads into the named source.

### Incoming Webhook Trigger

The `Croniq.Webhooks` host exposes tenant-scoped endpoints such as `POST /webhooks/{hookKey}`. Each hook references a job key and forwards request metadata into the event trigger shown above.

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

The webhook host validates the signature, enforces a per-hook rate limit, then enqueues a trigger with metadata (e.g., `metadata["invoiceId"] = ...`). Until GA, you can simulate the flow via custom controllers that call `WithEventTrigger` sources directly.

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

2. **List existing hooks** with `GET /tenants/{tenantId}/webhooks?environment=<tag>` to verify rate limits, metadata, and enablement before routing callers to the endpoint.
3. **Rotate secrets** with `POST /tenants/{tenantId}/webhooks/{hookKey}/rotate-secret?environment=<tag>`. Provide optional `gracePeriodSeconds` (default 24h) so the previous secret remains valid while upstream systems roll out the new key, and set `activateInSeconds` (up to seven days in the future) when you need a delayed cutover. The rotation response is the only time you see the plaintext secret—stash it in your secret manager immediately or pipe it into your vault automation.

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

4. **Disable or delete hooks** via `POST` (set `enabled:false`) for temporary pauses or `DELETE /tenants/{tenantId}/webhooks/{hookKey}?environment=<tag>` for permanent removal. Disabled hooks still show up in diagnostics; deleted hooks return `404` immediately.
5. **Audit usage** through telemetry (`Croniq.Webhooks.Ingress` spans) and, once wired up, the `WebhookIngressDeadLetter` table. Until then, structured logs remain the source of truth for per-hook activity.

> ⚠️ Signatures stay mandatory by default. To disable them for controlled scenarios, set `Croniq:Webhooks:Security:AllowUnsignedHooks=true` in configuration **and** pass `allowUnsigned=true` when calling the management API. `Croniq.Webhooks` logs a warning the first time an unsigned payload is accepted so you have an audit trail.

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

## Pausing & Resuming

```csharp
await cronScheduler.PauseAsync(jobKey);
await cronScheduler.ResumeAsync(jobKey);
```

Pausing keeps the trigger definition but suppresses new executions until resumed.

## Trigger Overlays via Configuration

Environment variables (e.g., `CRONIQ_JOBS__samples-HelloWorld__trigger`) override code-defined triggers, making it easy to tweak schedules per environment without redeploying.
