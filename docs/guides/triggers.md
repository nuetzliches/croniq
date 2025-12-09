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

### Incoming Webhook Trigger (planned)

Croniq.Api will expose tenant-scoped endpoints such as `POST /webhooks/{hookKey}`. Each hook references a job key and forwards request metadata into the event trigger above.

```http
POST /webhooks/invoice-paid HTTP/1.1
Host: api.croniq.local
X-Croniq-Key: crq_dev_local_sample
X-Croniq-Signature: sha256=...
Content-Type: application/json

{
    "invoiceId": "INV-2024-991",
    "tenant": "eu-shared",
    "amount": 349.0
}
```

Croniq validates the signature, enforces a per-hook rate limit, then enqueues a trigger with metadata (e.g., `metadata["invoiceId"] = ...`). Until the feature is GA, the same flow can be simulated via the `WithEventTrigger` source and a small proxy controller.

## Pausing & Resuming

```csharp
await cronScheduler.PauseAsync(jobKey);
await cronScheduler.ResumeAsync(jobKey);
```

Pausing keeps the trigger definition but suppresses new executions until resumed.

## Trigger Overlays via Configuration

Environment variables (e.g., `CRONIQ_JOBS__samples-HelloWorld__trigger`) override code-defined triggers, making it easy to tweak schedules per environment without redeploying.
