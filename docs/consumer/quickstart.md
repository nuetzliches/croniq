# Croniq Quickstart: Hello Croniq Job

This guide walks you through creating your first Croniq job, registering it with the Scheduler, and triggering it via the Minimal API. It assumes you already reviewed the architectural context in `CONCEPT.md` (sections 4–11).

> **Prerequisites**
>
> - .NET SDK `net10.0`
> - Access to the Croniq repository or NuGet feeds (once packages are published)
> - Docker (optional) for running the reference environment
>
> All commands are shown for Windows PowerShell / CMD; adapt paths for your OS if needed.
>
> Need the full Croniq reference environment? Follow [`docs/technical/devstack.md`](../technical/devstack.md) for compose profiles and helper scripts.

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
- API surface will evolve. Always check the latest signatures in `docs/technical` or `CONCEPT.md`.

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

Refer to `docs/technical/persistence.md` (to be added) for the exact schedule payload and validation rules.

## 6. Run Everything

```cmd
# Terminal 1
cd HelloCroniq.Api
 dotnet run
```

If you need the Croniq dev stack instead of a local SDK host, start it via the instructions in [`docs/technical/devstack.md`](../technical/devstack.md).

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

Watch the API logs; you should see the `HelloWorldJob` message. Logs, metrics, and traces will be emitted via Serilog + OpenTelemetry once configured (see `docs/technical/observability.md`, pending).

## 7. Clean Up & Next Steps

- Stop the API, tear down Docker resources (`docker compose down`) if used.
- Extend the job to read secrets via `ISecretProvider`, add retry policies, or push telemetry to your observability stack.
- Continue with:
  - [`configuration.md`](configuration.md) for tenant/environment options
  - [`policies.md`](policies.md) and [`triggers.md`](triggers.md) to tune job behavior
  - `docs/technical/job-registration.md` for internal startup flow and persistence sync details

Happy scheduling! Translate findings back into the documentation as you refine the workflow.
