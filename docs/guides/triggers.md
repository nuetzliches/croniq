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

### Incoming Webhook Trigger (planned)

The upcoming `Croniq.Webhooks` host exposes tenant-scoped endpoints such as `POST /webhooks/{hookKey}`. Each hook references a job key and forwards request metadata into the event trigger shown above.

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
3. **Rotate secrets** by calling the same `POST` endpoint with `secret` set to the new value. The response echoes the latest secret so you can update upstream systems before discarding the old key.
4. **Disable or delete hooks** via `POST` (set `enabled:false`) for temporary pauses or `DELETE /tenants/{tenantId}/webhooks/{hookKey}?environment=<tag>` for permanent removal. Disabled hooks still show up in diagnostics; deleted hooks return `404` immediately.
5. **Audit usage** through telemetry (`Croniq.Webhooks.Ingress` spans) and, once wired up, the `WebhookIngressDeadLetter` table. Until then, structured logs remain the source of truth for per-hook activity.

## Pausing & Resuming

```csharp
await cronScheduler.PauseAsync(jobKey);
await cronScheduler.ResumeAsync(jobKey);
```

Pausing keeps the trigger definition but suppresses new executions until resumed.

## Trigger Overlays via Configuration

Environment variables (e.g., `CRONIQ_JOBS__samples-HelloWorld__trigger`) override code-defined triggers, making it easy to tweak schedules per environment without redeploying.
