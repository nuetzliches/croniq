# Croniq Quickstart: Hello Croniq Job

This guide walks you through creating your first Croniq job, registering it with the Scheduler, and triggering it via the Minimal API. It assumes you already reviewed the architectural context in [`/deep-dive/architecture.md`](/deep-dive/architecture.md).

> **Prerequisites**
>
> - .NET SDK `net10.0`
> - Access to the Croniq repository or NuGet feeds (once packages are published)
> - Docker (optional) for running the reference environment
>
> All commands are shown for Windows PowerShell / CMD; adapt paths for your OS if needed.
>
> Need the full Croniq reference environment? Follow [`docs/deep-dive/devstack.md`](/deep-dive/devstack.md) for compose profiles and helper scripts.
>
> Still deciding how to secure the API? Read [`auth.md`](/guides/auth.md) for the consumer-facing guide to API keys vs OAuth2 before exposing your endpoints.

## 1. Create a Single Project

```cmd
mkdir HelloCroniq
cd HelloCroniq
dotnet new web -n HelloCroniq -f net10.0
```

This creates `HelloCroniq/HelloCroniq.csproj`, which will host both the Minimal API and your job implementations.

## 2. Reference Croniq Packages

Until official packages exist, you can reference local projects or NuGet prereleases.

```cmd
cd HelloCroniq
 dotnet add package Croniq.Api --version <latest>
 dotnet add package Croniq.Sdk --version <latest>
```

> Always install the latest stable version (see `AI_ASSISTANT_INSTRUCTIONS.md`). For local development before packages exist, add project references to `src/Croniq.Api` and `src/Croniq.Sdk` instead of NuGet packages.

## 3. Register a Fluent `IJob` Handler

Croniq supports a fluent registration model whenever you do not want to create a dedicated class type. Example inside `HelloCroniq/Program.cs`:

### Minimal handler

```csharp
using Croniq.Api;
using Croniq.Sdk;

var builder = WebApplication.CreateBuilder(args);

builder.Services.AddCroniq();

var helloWorldKey = JobKey.From("samples", "HelloWorld");

builder.Services.AddCroniqJob(helloWorldKey, job =>
  job.WithDescription("Logs a friendly greeting")
     .WithMetadata("owner", "quickstart")
     .Handle(async (context, cancellationToken) =>
     {
       context.Logger.LogInformation(
         "Hello from Croniq (single) JobKey={JobKey}",
         context.JobKey);
       await Task.CompletedTask;
     }));

var app = builder.Build();

app.MapCroniqManagementEndpoints(); // exposes /jobs/trigger, etc.

app.Run();
```

- `AddCroniqJob(JobKey jobKey, Action<IJobBuilder> configure)` accepts the fully composed key (namespace + job name + optional variant) as a single parameter. Use `JobKey.From(...)` or map existing metadata to keep naming deterministic.
- The fluent API surfaces multiple `Handle*` overloads if you need them, but Croniq expects most jobs to rely on a single `Handle` delegate and to report progress/state through the execution context (examples below).
- `builder.Services.AddCroniq()` reads every `CRONIQ_*` environment variable by default (e.g., `CRONIQ_ENDPOINT`, `CRONIQ_API_KEY`, optional `CRONIQ_TENANT`, `CRONIQ_ENV`), so you do not have to duplicate endpoint or key configuration in code.
- Configuration precedence and advanced scenarios are described in [`configuration.md`](configuration.md).
- API surface will evolve. Always check the latest signatures in `/deep-dive/` (start with `architecture.md`).

### Report progress from a single handler

```csharp
builder.Services.AddCroniqJob(helloWorldKey, job =>
  job.Handle(async (context, cancellationToken) =>
  {
    context.InitProgress(100);
    var processed = 0;

    try
    {
      for (; processed < 100; processed++)
      {
        await ProcessRecordAsync(processed, cancellationToken);
        context.ReportProgress(processed + 1);
      }

      context.Logger.LogInformation("Finished all {Total} records", processed);
    }
    catch (Exception ex)
    {
      context.Logger.LogError(ex, "Failed after {Processed} records", processed);
      throw; // bubble up so Croniq policies can retry or dead-letter
    }
  }));
```

- `InitProgress(total)` initializes the Croniq progress tracker; call it once per execution path (skip it when you bail out before processing begins).
- `ReportProgress(processed)` pushes the current count to the scheduler UI/logs.
- Log and rethrow exceptions so Croniq can execute retry/dead-letter policies.

### Emit custom waiting/running states

```csharp
builder.Services.AddCroniqJob(helloWorldKey, job =>
  job.Handle(async (context, cancellationToken) =>
  {
    if (!await PrerequisiteReadyAsync(cancellationToken))
    {
      return CustomState("waiting-on-dependency"); // defaults to JobState.Waiting
    }

    await ExecuteStepsAsync(cancellationToken);
    return CustomState("steps-finished", JobState.Finalized);
  }));
```

- `CustomState(string detail, JobState state = JobState.Waiting)` stores the Croniq core state plus your own descriptor. Keep descriptors deterministic so monitoring/search works well.
- Consider exposing a known list of descriptors per job (e.g., `waiting-on-dependency`, `step-1`, `step-2`, `finalized`) to make filtering easier in tooling; no prior registration is required by the runtime.

## 4. Class-Based Pattern (Alternative)

If you prefer a testable class or need constructor injection, implement `IJob` like this (in the same project) and register it via `AddCroniqJob<TJob>()`:

```csharp
using Croniq.Sdk;
using Microsoft.Extensions.Logging;

namespace HelloCroniq.Jobs;

[CroniqJob("samples", "HelloWorld")]
public sealed class HelloWorldJob : IJob
{
  public Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken)
  {
    context.Logger.LogInformation("Hello from Croniq (class)! JobKey={JobKey}", context.JobKey);
    return Task.CompletedTask;
  }
}

// Program.cs (additional registration)
builder.Services.AddCroniqJob<HelloWorldJob>();
```

```cmd
# Terminal 1
cd HelloCroniq
 dotnet run

# Terminal 2 (optional)

```

```cmd
curl -X POST https://localhost:5001/schedules \
     -H "Content-Type: application/json" \
     -H "X-Croniq-Key: <your-dev-key>" \
     -d "{
           \"jobKey\": \"dev-sandbox:dev-local:samples:HelloWorld\",
           \"cronExpression\": \"0 * * * * ?\",
           \"metadata\": { \"initiator\": \"quickstart\" }
         }"
```

Refer to `/deep-dive/persistence.md` (to be added) for the exact schedule payload and validation rules.

## 6. Run Everything

```cmd
# Terminal 1
cd HelloCroniq.Api
 dotnet run
```

If you need the Croniq dev stack instead of a local SDK host, start it via the instructions in [`docs/deep-dive/devstack.md`](/deep-dive/devstack.md).

Trigger the job manually:

```cmd
curl -X POST https://localhost:5001/jobs/trigger \
     -H "Content-Type: application/json" \
     -H "X-Croniq-Key: <your-dev-key>" \
     -d "{
           \"jobKey\": \"dev-sandbox:dev-local:samples:HelloWorld\",
           \"metadata\": { \"initiator\": \"manual\" }
         }"
```

Watch the API logs; you should see the `HelloWorldJob` message. Logs, metrics, and traces are emitted via Serilog + OpenTelemetry as soon as you point Croniq at an OTLP collector (details below).

## 7.1 Add Observability (Optional but Recommended)

1. Start the observability stack that ships with Croniq:

    ```cmd
    scripts\devstack-up.cmd --profile obs
    ```

    This launches Prometheus (`http://localhost:9090`), Tempo, and Grafana (`http://localhost:5610`, default credentials `admin/admin`).

    > **Tenant reminder**: Loki and Grafana share the tenant `croniq-devstack`. If you fork the stack, keep the `X-Scope-OrgID` header (in `infra/docker/observability/grafana/datasources/datasource.yml`) and the OTEL collector header (`infra/docker/observability/otel-collector-config.yaml`) in sync so Explore always queries the tenant that actually receives your logs. Labels exposed by the collector (`service_name`, `service_instance`, `environment`, `tenant`) make it easy to scope queries such as `{tenant="croniq-devstack", environment="dev"}`.

2. Configure your quickstart host to export telemetry. The `AddCroniqObservability` helper reads `Croniq:Observability` settings, so either add them to `appsettings.Development.json` or export environment variables before running `dotnet run`:

    ```cmd
    setx Croniq__Observability__OtlpEndpoint http://localhost:4317
    setx Croniq__Observability__OtlpProtocol grpc
    rem optional overrides
    setx Croniq__Core__EnvironmentTag dev
    setx Croniq__Core__TenantId samples
    ```

    Restart the application so the new environment variables take effect. The defaults already point at `otel-collector:4317` inside Docker, so these overrides are only needed when you run the app on your host machine.

3. Trigger your job again. Within a few seconds you can:

    - Open Grafana ▸ Dashboards ▸ _Croniq Scheduler Health_ or _Croniq API Gateway_ to view the panels provisioned from `infra/docker/observability/grafana/dashboards/` (they refresh every 30s).
    - Inspect traces under Grafana ▸ Explore ▸ Tempo, filtering by `service.name="Croniq.Api"`.
    - Check Prometheus ▸ Alerts to see the built-in alerts from `infra/monitoring/rules/scheduler-alerts.yaml`. Alerts fire when dead letters, misfires, queue depth, or latency breach their thresholds (`CroniqDeadLettersHigh`, `CroniqMisfireBurst`, `CroniqQueueDepthHigh`, `CroniqLatencyP95High`, `CroniqJobFailures`).

4. Deploying to your own observability stack? Copy the dashboard JSON + rule file into your Grafana/Prometheus setup and keep the datasource UIDs (`prometheus`, `tempo`) consistent. See [`docs/deep-dive/observability.md`](/deep-dive/observability.md#dashboards--alerts) for detailed instructions.

## 7. Clean Up & Next Steps

- Stop the API, tear down Docker resources (`docker compose down`) if used.
- Extend the job to read secrets via `ISecretProvider`, add retry policies, or push telemetry to your observability stack.
- Continue with:
  - [`configuration.md`](/introduction/configuration.md) for tenant/environment options
  - [`auth.md`](/guides/auth.md) to switch between API key and OIDC caller flows
  - [`policies.md`](/guides/policies.md) and [`triggers.md`](/guides/triggers.md) to tune job behavior
  - `/deep-dive/job-registration.md` for internal startup flow and persistence sync details
- Hit [`troubleshooting.md`](/ops/troubleshooting.md) if any of the steps above fail or you suspect dev stack issues

Happy scheduling! Translate findings back into the documentation as you refine the workflow.
