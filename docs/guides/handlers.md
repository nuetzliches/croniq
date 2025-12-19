# Croniq Handler Patterns

Croniq executes work through `IJob.ExecuteAsync(IJobExecutionContext context, CancellationToken token)`. You can register handlers inline or implement `IJob` directly for more complex scenarios.

## Inline Handler (delegate)

```csharp
builder.Services.AddCroniqJob("samples", "smoke", (context, cancellationToken) =>
{
    context.Logger.LogInformation("Hello from {JobKey}", context.JobKey);
    return Task.CompletedTask;
});
```

Need DI services inside the handler? Use the overload that receives `IServiceProvider`:

```csharp
builder.Services.AddCroniqJob("samples", "smoke", (services, context, cancellationToken) =>
{
    var env = services.GetRequiredService<IHostEnvironment>();
    context.Logger.LogInformation("Environment: {Environment}", env.EnvironmentName);
    return Task.CompletedTask;
});
```

## Class-based Handler

```csharp
[CroniqJob("samples", "smoke")]
public sealed class SmokeJob : IJob
{
    private readonly IHostEnvironment _env;

    public SmokeJob(IHostEnvironment env)
    {
        _env = env;
    }

    public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken)
    {
        context.Logger.LogInformation("Environment: {Environment}", _env.EnvironmentName);
        return Task.CompletedTask;
    }
}

builder.Services.AddCroniqJob<SmokeJob>();
```

## Modeling Batch or Long-running Work

Croniq does not ship a batch-handler DSL yet. For now, model batching inside the handler (paginate from a data source, process a bounded number of items per execution, and persist progress yourself).

## Next Steps

- Configure schedules via [`triggers.md`](./triggers.md).
- Configure retries, timeouts, and quotas via [`policies.md`](./policies.md).
