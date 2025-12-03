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

## Pausing & Resuming

```csharp
await cronScheduler.PauseAsync(jobKey);
await cronScheduler.ResumeAsync(jobKey);
```

Pausing keeps the trigger definition but suppresses new executions until resumed.

## Trigger Overlays via Configuration

Environment variables (e.g., `CRONIQ_JOBS__samples-HelloWorld__trigger`) override code-defined triggers, making it easy to tweak schedules per environment without redeploying.
