# Croniq Runner SDK (.NET)

Lightweight .NET runner SDK with gRPC streaming (primary) and HTTP polling fallback.

## Usage (Hosted Service)

```csharp
using Croniq.Runner;

builder.Services.AddCroniqRunnerHostedService(options =>
{
    options.Config = RunnerConfig.FromEnvironment() with
    {
        HeartbeatInterval = TimeSpan.FromSeconds(15)
    };

    options.OnExecute("demo-job", async (context, payload, logger, cancellationToken) =>
    {
        logger.Info("execution started", new Dictionary<string, object?>
        {
            ["executionId"] = context.ExecutionId,
            ["jobKey"] = context.JobKey
        });

        await Task.Delay(250, cancellationToken);

        logger.Info("execution completed", new Dictionary<string, object?>
        {
            ["executionId"] = context.ExecutionId
        });
    });
});
```

## Usage (Manual Start/Drain)

```csharp
var config = RunnerConfig.FromEnvironment();
var runner = new CroniqRunner(config);

runner.OnExecute("demo-job", async (context, payload, logger, cancellationToken) =>
{
    await Task.Delay(100, cancellationToken);
});

await runner.StartAsync();
```

## Configuration

Use `RunnerConfig.FromEnvironment()` to load:

- Required: `CRONIQ_API_BASEURL`, `CRONIQ_TENANT_ID`, `CRONIQ_ENVIRONMENT`, `CRONIQ_RUNNER_ID`, and exactly one of `CRONIQ_API_KEY` or `CRONIQ_BEARER_TOKEN`.
- Optional transport: `CRONIQ_GRPC_BASEURL`, `CRONIQ_TRANSPORT_MODE` (`auto|grpc|polling`), `CRONIQ_ALLOW_TEST_EXECUTIONS`.
- Optional tuning: `CRONIQ_POLL_BATCH_SIZE`, `CRONIQ_POLL_WAIT_MS`, `CRONIQ_REQUEST_TIMEOUT_MS`, `CRONIQ_RENEW_LEAD_MS`,
  `CRONIQ_RETRY_BASE_MS`, `CRONIQ_RETRY_MAX_MS`, `CRONIQ_RETRY_MAX_ATTEMPTS`, `CRONIQ_MAX_INFLIGHT`, `CRONIQ_CAPABILITIES`,
  `CRONIQ_RUNNER_INSTANCE_ID`.
