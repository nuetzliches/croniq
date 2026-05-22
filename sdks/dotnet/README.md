# Croniq Runner SDK for .NET

[![NuGet](https://img.shields.io/nuget/v/Croniq.Runner.Sdk.svg)](https://www.nuget.org/packages/Croniq.Runner.Sdk)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Build job execution runners for [Croniq](https://github.com/nuetzliches/croniq) in .NET. The SDK polls a Croniq server for work, dispatches typed handlers, streams structured logs back, and reports completion — all with idiomatic Generic Host integration.

## Install

```sh
dotnet add package Croniq.Runner.Sdk
# optional: tracing + metrics
dotnet add package Croniq.Runner.Sdk.OpenTelemetry
```

Target frameworks: `net8.0`, `net10.0`.

## Quick start (Worker Service / .NET 10 top-level statements)

```csharp
using Croniq.Runner.Sdk;
using Croniq.Runner.Sdk.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

var builder = Host.CreateApplicationBuilder(args);

builder.Services
    .AddCroniqRunner(builder.Configuration.GetSection(CroniqRunnerOptions.SectionName))
    .AddCroniqJob("hello:world", async (ctx, ct) =>
    {
        ctx.Logger.LogInformation("Hello from {Job} (attempt {Attempt})", ctx.JobKey, ctx.Attempt);
        await Task.Delay(TimeSpan.FromSeconds(1), ct);
    });

await builder.Build().RunAsync();
```

`appsettings.json`:

```json
{
  "Croniq": {
    "Runner": {
      "ServerUrl": "http://localhost:4000",
      "RunnerIdPrefix": "demo-runner",
      "ApiKey": "croniq_…",
      "MaxInflight": 5,
      "Capabilities": [ "demo" ],
      "Tags": [ "lang=dotnet", "env=dev" ]
    }
  }
}
```

## Features

- **Generic Host integration** — `IHostedService` adapter, graceful shutdown via `IHostApplicationLifetime`.
- **Two handler styles**:
  - delegate: `AddCroniqJob("key", async (ctx, ct) => …)`
  - DI-friendly interface: `AddCroniqJob<MyHandler>("key")` with `ICroniqJobHandler`
- **Server-side cancellation** — `PollResponse.cancel` is wired into per-execution `CancellationToken`.
- **Streaming log writer** — `ctx.LogWriter` backs onto `System.Threading.Channels` with backpressure, batching (32 events / 200 ms / max 100 per POST), drain-before-ack.
- **Self-registration** — `AddCroniqJob<T>("key", schedule: "5m")` calls `POST /v1/jobs/register` on startup.
- **Health checks** — `services.AddHealthChecks().AddCroniqRunnerHealthCheck()`.
- **OpenTelemetry** — opt-in via `tracerBuilder.AddCroniqRunnerInstrumentation()` (separate package).
- **Shell-exec decoder** — handles DSL `runner shell { … }` / `runner exec { … }` jobs by decoding `__runner_exec` metadata and spawning a subprocess; stdout/stderr is streamed via the log writer.
- **AOT-compatible** — strongly-typed JSON via source-generated `JsonSerializerContext`.

## Capabilities vs Tags

A common pitfall: **don't put implementation details into capabilities**. Capabilities drive job routing (`require`/`prefer` in the Croniqfile). Tags are filter-only — for the UI and operations, not routing.

| Good capability | Bad capability |
|---|---|
| `billing`, `reporting`, `gpu`, `sandboxed` | `dotnet`, `python`, `linux-x64` |

If your runner is .NET-based, put that into **tags** (`lang=dotnet`, `platform=linux-x64`) so a future Rust- or Python-runner with the same business capabilities can take over without rewriting Croniqfile entries.

## DI-friendly handler example

```csharp
public sealed class BillingInvoiceHandler(
    ILogger<BillingInvoiceHandler> logger,
    IInvoiceService invoices) : ICroniqJobHandler
{
    public async Task HandleAsync(ExecutionContext ctx, CancellationToken cancellationToken)
    {
        var customerId = ctx.Metadata.GetProperty("customer_id").GetString();
        logger.LogInformation("Generating invoice for {Customer}", customerId);

        await using var writer = ctx.LogWriter;
        await foreach (var line in invoices.GenerateAsync(customerId!, cancellationToken))
            await writer.WriteAsync(LogLevel.Information, line, ct: cancellationToken);
    }
}

builder.Services.AddCroniqRunner(...)
    .AddCroniqJob<BillingInvoiceHandler>("billing:invoice", schedule: "5m");
```

## OpenTelemetry

```csharp
builder.Services.AddOpenTelemetry()
    .WithTracing(t => t.AddCroniqRunnerInstrumentation().AddOtlpExporter())
    .WithMetrics(m => m.AddCroniqRunnerInstrumentation().AddOtlpExporter());
```

Span name: `croniq.execute {job_key}`. Standard attributes: `croniq.job.key`, `croniq.execution.id`, `croniq.execution.attempt`, `croniq.runner.id`, `croniq.execution.outcome`.

## Compatibility matrix

| SDK Version | Croniq Server (min) | Croniq Server (max tested) |
|-------------|---------------------|----------------------------|
| 0.1.x       | 0.14.0              | 0.14.0                     |

## Pre-release feed (GitHub Packages)

Add to `nuget.config`:

```xml
<add key="croniq-github" value="https://nuget.pkg.github.com/nuetzliches/index.json" />
```

A GitHub PAT with `read:packages` scope is required for restore.

## License

Dual-licensed under MIT OR Apache-2.0. See [LICENSE-MIT](../../LICENSE-MIT) and [LICENSE-APACHE](../../LICENSE-APACHE).
