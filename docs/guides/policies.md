# Croniq Job Policies

Policies let you describe operational behavior independently from handler logic. Attach them via the fluent builder after defining handlers.

## Concurrency

```csharp
job.WithConcurrency(options =>
{
    options.MaxParallelRuns = 2;
    options.QueueOverflow = ConcurrencyOverflowPolicy.Queue;
});
```

Set `MaxParallelRuns` to limit how many executions overlap per job key. Overflow policy chooses between queueing, dropping, or replacing pending work.

## Retries

```csharp
job.WithRetry(options =>
{
    options.MaxAttempts = 5;
    options.Backoff = RetryBackoff.Exponential(TimeSpan.FromSeconds(5));
});
```

Exponential backoff protects downstream systems. Use `RetryPredicate` when only certain exceptions should retry.

## Timeouts

```csharp
job.WithTimeout(TimeSpan.FromMinutes(2));
```

Croniq cancels the handler via `CancellationToken` when the timeout elapses.

## Dead Letter Routing

```csharp
job.WithDeadLetter(queue =>
{
    queue.Target = DeadLetterTarget.Storage("cron-failures");
});
```

Store unprocessable payloads for later inspection. Integration depends on your configured provider (e.g., Azure Storage, SQS).

## Idempotency Tokens

```csharp
job.WithIdempotency(id =>
{
    id.ResolveFrom(context => context.Metadata["orderId"]);
});
```

Prevents duplicate execution when the same payload arrives multiple times.

## Composition Order

Policies apply in the order you register them. A typical chain:

```csharp
builder.Services.AddCroniqJob(jobKey, job =>
    job.Handle(...)
       .WithConcurrency(...)
       .WithRetry(...)
       .WithTimeout(...));
```

## Diagnostics

- Enable structured logging via `AddCroniq(options => options.Logging = CroniqLogging.Verbose);`
- Use `context.Telemetry` inside handlers to emit custom traces.
