# Croniq Handler Patterns

Croniq’s fluent builder exposes multiple handler types so you can align runtime behavior with your job contract. All handler delegates ultimately flow into the `IJob.ExecuteAsync(IJobExecutionContext context, CancellationToken token)` contract—naming remains consistent with the SDK interface.

## 1. Single Item Handler (`Handle`)

```csharp
builder.Services.AddCroniqJob(jobKey, job =>
    job.Handle(async (context, cancellationToken) =>
    {
        // One execution per scheduled fire
        await DoWorkAsync(context, cancellationToken);
    }));
```

Use this for traditional Cron-like jobs where each trigger executes exactly once.

## 2. Batch Handler (`HandleBatch`)

```csharp
job.HandleBatch(async (batch, context, cancellationToken) =>
{
    foreach (var item in batch)
    {
        await ProcessAsync(item, cancellationToken);
    }
});
```

The scheduler groups queued work items (size configurable in options) and passes them to the delegate. Return partial completion information via metadata if needed.

## 3. Stateful Handler (`HandleWithState`)

```csharp
job.HandleWithState(async (state, context, cancellationToken) =>
{
    if (state.TryGetValue("resumeToken", out var resume))
    {
        await ResumeAsync(resume, cancellationToken);
    }
});
```

The `state` dictionary captures custom checkpoints (e.g., last processed ID). Use it for long-running tasks that must pause/resume between triggers.

## 4. Combining Handlers

Handlers can be chained. For example:

```csharp
builder.Services.AddCroniqJob(jobKey, job =>
    job.Handle(...)
       .HandleBatch(...)
       .HandleWithState(...));
```

Only the delegates you configure will run; omit the ones you do not need.

## 5. Relation to `IJob`

Even when using the fluent API, Croniq materializes an internal `IJob` implementation that executes your handlers in the declared order. If you implement `IJob` yourself, keep the method name `ExecuteAsync` to remain compatible.

## 6. Next Steps

- Configure concurrency, retries, and timeout policies via [`policies.md`](policies.md).
- Learn how to attach different trigger types (cron, interval, ad-hoc) via [`triggers.md`](triggers.md).
