using Croniq.Runner.Sdk;
using Croniq.Runner.Sdk.Configuration;
using Croniq.Runner.Sdk.DependencyInjection;
using Croniq.Runner.Sdk.HealthChecks;
using Croniq.Runner.Sdk.OpenTelemetry;

using CroniqRunner.Demo;

using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;

using OpenTelemetry.Metrics;
using OpenTelemetry.Resources;
using OpenTelemetry.Trace;

var builder = Host.CreateApplicationBuilder(args);

// Bind Croniq:Runner section (appsettings.json + ENV overrides like CRONIQ__RUNNER__APIKEY)
builder.Services
    .AddCroniqRunner(builder.Configuration.GetSection(CroniqRunnerOptions.SectionName))

    // 1) Delegate handler — minimal, lambda-style
    .AddCroniqJob("hello:world", async (ctx, ct) =>
    {
        ctx.Logger.LogInformation("Hello from {Job} (attempt {Attempt})", ctx.JobKey, ctx.Attempt);

        // Optional: stream lines back to the Croniq UI via the lazy log writer
        await ctx.LogWriter.WriteAsync(LogLevel.Information, "step 1: greeting", cancellationToken: ct);
        await Task.Delay(TimeSpan.FromMilliseconds(250), ct);
        await ctx.LogWriter.WriteAsync(LogLevel.Information, "step 2: done", cancellationToken: ct);
    })

    // 2) Interface handler with DI — auto-registers schedule on startup
    .AddCroniqJob<BillingInvoiceHandler>("billing:invoice", schedule: "5m")

    // 3) Shell-exec default — picks up DSL `runner shell { ... }` / `runner exec { ... }` jobs
    .AddCroniqShellHandler();

builder.Services.AddSingleton<IInvoiceService, FakeInvoiceService>();

builder.Services.AddHealthChecks()
    .AddCroniqRunnerHealthCheck();

builder.Services.AddOpenTelemetry()
    .ConfigureResource(r => r.AddService("croniq-runner-demo"))
    .WithTracing(t => t
        .AddCroniqRunnerInstrumentation()
        .AddOtlpExporter())
    .WithMetrics(m => m
        .AddCroniqRunnerInstrumentation()
        .AddOtlpExporter());

await builder.Build().RunAsync();
