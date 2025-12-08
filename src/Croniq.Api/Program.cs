using System.Reflection;
using Croniq.Api;
using Croniq.Core;
using OpenTelemetry.Metrics;
using OpenTelemetry.Trace;

var builder = WebApplication.CreateBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqApiRateLimiter();

var otelBuilder = builder.Services.AddCroniqObservability(
    builder.Configuration,
    builder.Logging,
    "Croniq.Api",
    options => options.ServiceVersion = Assembly.GetExecutingAssembly().GetName().Version?.ToString() ?? "dev");

otelBuilder.WithTracing(tracing =>
{
    tracing
        .AddAspNetCoreInstrumentation(options => options.RecordException = true)
        .AddHttpClientInstrumentation()
        .AddSource("Croniq.Core")
        .AddSource("Croniq.Api.Trigger");
});

otelBuilder.WithMetrics(metrics =>
{
    metrics
        .AddAspNetCoreInstrumentation()
        .AddHttpClientInstrumentation()
        .AddRuntimeInstrumentation()
        .AddMeter("Croniq.Core");
});

var app = builder.Build();

app.UseCroniqApi();

app.Run();
