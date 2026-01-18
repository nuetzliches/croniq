# Croniq Triggers

Triggers decide when a job key should run. Croniq currently supports cron-based schedules. You can create them via the API/gRPC or seed them at worker startup (config or fluent).

## Cron Expressions

Croniq uses 6-field cron expressions (seconds precision) with an optional year field. Examples:

```csharp
var everyFiveMinutes = "0 */5 * * * *";
var weekdaysAtNine = "0 0 9 * * MON-FRI";
```

Croniq also supports the special expression `@once` (alias `once`) for a single execution. When used, Croniq schedules exactly one run at `StartAtUtc` if provided, otherwise it fires immediately.

Schedules run in UTC by default. Persisted triggers store the cron expression plus optional start/end bounds.

## Calendars

Croniq supports schedule calendars as separate entities that include or exclude candidate fire times. Attach a calendar by setting `calendarId` on the schedule upsert and manage calendar definitions via `/tenants/{tenantId}/calendars`. For rule types, evaluation semantics, and constraints, see `docs/deep-dive/designs/schedule-calendars.md`.

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

| Field          | Required | Notes                                                                                                                                                                   |
| -------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TriggerId      | No       | Defaults to `{JobKey}:{base64url(cronExpression)}` (and `:{base64url(timeZoneId)}` when provided). If the result exceeds 512 chars, Croniq uses `JobKey:hash-<sha256>`. |
| JobKey         | Yes      | Must follow the Croniq job key format (`namespace:name[:variant]`). Tenant/environment are taken from the hosting scope, not from the job key.                          |
| CronExpression | Yes      | 6-field cron expression (optional year), or `@once` for a one-off trigger.                                                                                              |
| TimeZoneId     | No       | Time zone id for schedule evaluation; defaults to `UTC` when omitted or invalid.                                                                                        |
| CalendarId     | No       | Optional calendar definition id to filter occurrences; the calendar must exist in the same tenant/environment.                                                          |
| StartAtUtc     | No       | Optional UTC start bound (ISO-8601).                                                                                                                                    |
| EndAtUtc       | No       | Optional UTC end bound (ISO-8601).                                                                                                                                      |
| Enabled        | No       | Defaults to `true`.                                                                                                                                                     |
| ManagedBy      | No       | Required when `Croniq:Seeding:Mode=ForceUpdate`; also stored as `metadata.managedBy`.                                                                                   |
| Metadata       | No       | String dictionary stored with the trigger definition and exposed via `IJobExecutionContext.Metadata`.                                                                   |

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

## Webhook Triggers

Webhook ingress is documented separately in [`webhooks.md`](./webhooks.md). The guide covers the ingress endpoints, lifecycle management, and security guidance for signing and rate limiting.

## Enable or Disable Schedules

Update a schedule via the API (or config/seeded triggers) and set `enabled=false` to pause it. Re-enable by setting `enabled=true` or delete the schedule to remove it entirely.

## Configuration Overrides

`Croniq:Triggers` uses the normal configuration pipeline, so environment variables can override JSON values when you need per-environment schedules.

> **Learn more:** See the deep dives on [job registration](../deep-dive/job-registration.md) and [schedule calendars](../deep-dive/designs/schedule-calendars.md) for the registration pipeline and rule evaluation semantics.
