# Croniq Webhooks

## Overview

Croniq.Webhooks lets external systems trigger Croniq jobs via HTTP. This guide focuses on ingress endpoints, webhook lifecycle management, and caller security. For cron schedules and @once triggers, see [`triggers.md`](./triggers.md).

## API Surface

- Ingress: `POST /tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}`
- Manual invoke (admin): `POST /tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}/invoke`
- Management: `POST/GET/DELETE /tenants/{tenantId}/webhooks?environment=<tag>` + `POST /tenants/{tenantId}/webhooks/{hookKey}/rotate-secret?environment=<tag>`
- Diagnostics: dead letters under `/tenants/{tenantId}/webhooks/deadletters` and IP rules under `/tenants/{tenantId}/webhooks/{hookKey}/ip-rules`
- Activity timeline: `GET /tenants/{tenantId}/webhooks/activity?environment=<tag>&fromUtc=<iso>&toUtc=<iso>&hookKeys=a,b&jobKeys=c,d&limit=200`
- Activity summary: `GET /tenants/{tenantId}/webhooks/activity/summary?environment=<tag>&fromUtc=<iso>&toUtc=<iso>&bucketMinutes=60`
- Remote health (proxy): `GET /tenants/{tenantId}/webhooks/remote/health?environment=<tag>` (API host checks the remote `/health` when `Croniq:Webhooks:Mode=Remote`)
- DMZ ingress relay/streaming: see [`docs/deep-dive/designs/dmz-ingress-remote-webhooks.md`](../deep-dive/designs/dmz-ingress-remote-webhooks.md)

Activity timeline entries include a `source` field (`ingress` or `invoke`) so operators can distinguish manual invokes from inbound deliveries.

### Activity SLA (preview)

- Availability: activity endpoints require SqlServer/Postgres webhook persistence; in-memory mode returns `503 webhook-activity-unavailable`.
- Freshness: TriggerJob records successful dispatches (failures surface via dead letters), StoreOnly reflects relay acknowledgements. The SSE stream polls every 5 seconds, so expect short (seconds) staleness.
- Defaults: timeline `limit` defaults to 200 (max 500). Summary defaults to a 24h window with 60-minute buckets; max window 31 days, max bucket 24 hours.

### Activity statuses and summary fields

Activity timeline entries include `status` values that map to the ingress lifecycle:

- `pending`: ingress stored but not yet leased by a relay worker
- `leased`: relay worker has leased the ingress event but has not confirmed delivery
- `success`: delivery completed without errors
- `warning`: delivery completed after retries (attempt count greater than 1)
- `failed`: delivery failed or an entry represents a dead letter

Activity summary buckets include `totalCount`, `errorCount`, `warningCount`, `pendingCount`, `leasedCount`, `deadLetterCount`, and optional `p95LatencyMs`.

> Note: UI retry/outcome labeling currently infers retries from `warning` status. A dedicated `attempts` field is not yet exposed in the activity timeline response; see UI backlog entry for adding it.

For relay flow details that generate pending/leased transitions, see [`dmz-ingress-remote-webhooks.md`](../deep-dive/designs/dmz-ingress-remote-webhooks.md).

## Ingress Endpoint

The `Croniq.Webhooks` host exposes tenant-scoped endpoints such as `POST /tenants/{tenantId}/environments/{environmentTag}/webhooks/{hookKey}`. Each hook references a job key and forwards request metadata into the job execution.

```http
POST /tenants/default/environments/dev/webhooks/invoice-paid HTTP/1.1
Host: hooks.croniq.local
X-Croniq-Signature: sha256=...
Content-Type: application/json

{
    "invoiceId": "INV-2024-991",
    "tenant": "eu-shared",
    "amount": 349.0
}
```

The webhook host validates the signature, enforces a per-hook rate limit, then enqueues a trigger with metadata (for example, `payload:invoiceId`).

EnvironmentTag is part of the webhook URL and partitions hooks. A webhook can only trigger jobs in the same tenant/environment scope; to target another environment, create a separate hook under that environment.

## Configuration

Sample configuration (`appsettings.Development.json`) wired up in `Croniq.Sample.ApiHost`:

::: code-group

```json [appsettings.Development.json]
{
  "Croniq": {
    "Webhooks": {
      "RequestsPerMinute": 30,
      "Endpoints": [
        {
          "HookKey": "invoice-paid",
          "JobKey": "samples:smoke",
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

```dotenv [.env]
CRONIQ_WEBHOOKS_REQUESTS_PER_MINUTE=30
CRONIQ_WEBHOOKS_ENDPOINTS_0_HOOK_KEY=invoice-paid
CRONIQ_WEBHOOKS_ENDPOINTS_0_JOB_KEY=samples:smoke
CRONIQ_WEBHOOKS_ENDPOINTS_0_SECRET=dev-webhook-secret
CRONIQ_WEBHOOKS_ENDPOINTS_0_REQUIRE_SIGNATURE=true
CRONIQ_WEBHOOKS_ENDPOINTS_0_METADATA_source=sample
CRONIQ_WEBHOOKS_ENDPOINTS_0_METADATA_type=invoice
```

```powershell [PowerShell]
$Env:Croniq__Webhooks__RequestsPerMinute = "30"
$Env:Croniq__Webhooks__Endpoints__0__HookKey = "invoice-paid"
$Env:Croniq__Webhooks__Endpoints__0__JobKey = "samples:smoke"
$Env:Croniq__Webhooks__Endpoints__0__Secret = "dev-webhook-secret"
$Env:Croniq__Webhooks__Endpoints__0__RequireSignature = "true"
$Env:Croniq__Webhooks__Endpoints__0__Metadata__source = "sample"
$Env:Croniq__Webhooks__Endpoints__0__Metadata__type = "invoice"
```

:::

This exposes `POST /tenants/default/environments/dev/webhooks/invoice-paid` locally. The sample job logs every invocation, and metadata keys such as `payload:invoiceId` become available via `IJobExecutionContext.Metadata`.

For DMZ deployments with `Croniq:Webhooks:Mode=Remote`, keep `Croniq:Webhooks:Remote:EnableRelay=true` on the worker host (the host that registers jobs) and disable it on the API host. The API host still needs the remote config to manage hooks and invoke `/invoke`, but it should not execute ingress relay itself.

If the remote admin API and ingress endpoints are hosted on different domains, set `Croniq:Webhooks:Remote:IngressBaseUrl` on the API host. When omitted, the API host uses `Croniq:Webhooks:Remote:BaseUrl` for both admin and ingress relay/invoke calls.

Example request against the sample host:

```bash
curl -X POST http://localhost:5199/tenants/default/environments/dev/webhooks/invoice-paid \
  -H "Content-Type: application/json" \
  -H "X-Croniq-Signature: $(python - <<'PY'
import hmac, hashlib, json
secret = b'dev-webhook-secret'
payload = json.dumps({\"invoiceId\": \"INV-2024-991\", \"amount\": 349.0})
sig = hmac.new(secret, payload.encode(), hashlib.sha256).hexdigest()
print(f\"sha256={sig}\")
PY
)" \
  -d '{"invoiceId":"INV-2024-991","amount":349.0}'
```

`Croniq.Webhooks` recomputes the `sha256=<hex>` signature server-side, applies the per-hook rate limit (default 30 rpm above), and then dispatches the configured job.

## Webhook Lifecycle

1. **Create or update hooks** via the management API: `POST /tenants/{tenantId}/webhooks?environment=<tag>` with a body such as:

   ```json
   {
     "hookKey": "invoice-paid",
     "jobKey": "samples:smoke",
     "secret": "dev-webhook-secret",
     "requestsPerMinute": 30,
     "metadata": { "source": "sample" }
   }
   ```

   Dev stacks can still fall back to `Croniq:Webhooks` config, but production tenants should rely on the API so secrets are persisted in SqlServer or Postgres.

2. **Fetch capabilities** with `GET /tenants/{tenantId}/webhooks/capabilities?environment=<tag>` to learn the default `requestsPerMinute` and whether unsigned hooks can be enabled before setting `allowUnsigned=true` in the payload.

3. **List existing hooks** with `GET /tenants/{tenantId}/webhooks?environment=<tag>` to verify rate limits, metadata, and enablement before routing callers to the endpoint.

4. **Rotate secrets** with `POST /tenants/{tenantId}/webhooks/{hookKey}/rotate-secret?environment=<tag>`. Provide optional `gracePeriodSeconds` (default 24h) so the previous secret remains valid while upstream systems roll out the new key, and set `activateInSeconds` (up to seven days in the future) when you need a delayed cutover. The rotation response is the only time you see the plaintext secret; stash it in your secret manager immediately or pipe it into your vault automation.

   ::: details PowerShell helper (scripts/webhook-rotate-secret.ps1)

   ```powershell
   scripts/webhook-rotate-secret.ps1 `
     -TenantId tenant-a `
     -Environment dev `
     -HookKey invoice-paid `
     -ActivateInSeconds 900 `
     -GracePeriodSeconds 86400 `
     -Notes "rotated via aspire devstack"
   ```

   :::

   The script prints the activation/expires timestamps and the new secret so you can capture it immediately.

5. **Disable or delete hooks** via `POST` (set `enabled:false`) for temporary pauses or `DELETE /tenants/{tenantId}/webhooks/{hookKey}?environment=<tag>` for permanent removal. Disabled hooks still show up in diagnostics; deleted hooks return `404` immediately.

## Operations & Monitoring

Audit usage through telemetry (`Croniq.Webhooks.Ingress` spans) and the `croniq.WebhookDeadLetters` table when dead-lettering is enabled. Configure with `Croniq:Webhooks:DeadLetter:Enabled` and `Croniq:Webhooks:DeadLetter:RetentionDays`. In in-memory mode, structured logs remain the source of truth for per-hook activity.

When using SqlServer or Postgres webhook persistence, the activity endpoints provide a timeline and bucketed summary sourced from ingress events and dead letters. If the activity store is not configured (for example in-memory mode), the endpoints return `503 webhook-activity-unavailable`.

## Webhook Security Guidance

::: warning Signatures
Signatures stay mandatory by default. Disable them only when the capabilities endpoint reports `allowUnsignedHooks=true` and you pass `allowUnsigned=true` in the management API payload. In local mode, `Croniq:Webhooks:Security:AllowUnsignedHooks=true` must be set as well. `Croniq.Webhooks` logs a warning the first time an unsigned payload is accepted so you have an audit trail.
:::

Consumers that call Croniq's webhook ingress should implement the following safeguards so every trigger remains tamper-proof and traceable.

### Signature generation

`X-Croniq-Signature` is `sha256=<hex>` where `<hex>` is the lowercase HMAC-SHA256 digest of the UTF-8 request body using the shared webhook secret. Example implementations:

::: code-group

```csharp [C#]
using var hmac = new HMACSHA256(Encoding.UTF8.GetBytes(secret));
var payload = JsonSerializer.Serialize(body);
var signature = "sha256=" + Convert.ToHexString(hmac.ComputeHash(Encoding.UTF8.GetBytes(payload))).ToLowerInvariant();
request.Headers.Add("X-Croniq-Signature", signature);
```

```ts [TypeScript]
import crypto from "node:crypto";

const payload = JSON.stringify(body);
const digest = crypto
  .createHmac("sha256", secret)
  .update(payload, "utf8")
  .digest("hex");
request.set("X-Croniq-Signature", `sha256=${digest}`);
```

```go [Go]
h := hmac.New(sha256.New, []byte(secret))
h.Write(bodyBytes)
signature := fmt.Sprintf("sha256=%x", h.Sum(nil))
req.Header.Set("X-Croniq-Signature", signature)
```

:::

Treat secrets as credentials: read them from your secret manager at runtime, never check them into source control, and rotate them via the management API runbook above.

### Recommended headers

- `Content-Type: application/json` (or the actual MIME type) so Croniq can parse payload metadata consistently.
- `User-Agent` identifying the workload (`sap-billing-forwarder/2.4`).
- `X-Croniq-Tenant` / `X-Croniq-Environment` are not required; the hook already maps to a tenant/environment. Add informational metadata via `payload:*` fields if you need extra routing context.

### Backoff and error handling

- `403 ip-blocked`: your source IP is not listed. Cross-check the allow list and update it via the API or `WebhookIpRuleClient` before retrying.
- `401 signature-invalid`: regenerate the signature using the newest secret, confirm there is no whitespace/double encoding, and check whether a rotation just occurred (see `WebhookEndpointEvents`).
- `429 rate-limit`: respect the `Retry-After` header. Croniq's fixed window allows short bursts but will throttle noisy callers.

### Secret rotation

- Subscribe to your CMDB/secret manager so rotations propagate to caller workloads within the grace window returned by `POST .../rotate-secret`.
- After rotating, send health-check payloads using both the old and new secrets to confirm Croniq accepts them until the grace period expires.
- Keep rotation notes (`notes` field) descriptive; Croniq surfaces them in `WebhookEndpointEvents` to speed up incident reviews.

## Related Guides

- [`auth.md`](./auth.md) for API keys vs bearer tokens when managing webhooks.
- [`triggers.md`](./triggers.md) for cron schedules and `@once` triggers.

> **Learn more:** Dive into [architecture.md](../deep-dive/architecture.md#webhook-trigger-surface), the [DMZ ingress design](../deep-dive/designs/dmz-ingress-remote-webhooks.md), and [secret rotation](../deep-dive/designs/webhook-secret-rotation.md) for operational internals.
