using System.Reflection;
using Croniq.Core;
using Croniq.Webhooks;
using OpenTelemetry.Metrics;
using OpenTelemetry.Trace;

var builder = WebApplication.CreateBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.AddCroniqWebhookServices(builder.Configuration);
builder.Services.AddCroniqWebhookRateLimiter();

var otelBuilder = builder.Services.AddCroniqObservability(
    builder.Configuration,
    builder.Logging,
    "Croniq.Webhooks",
    options => options.ServiceVersion = Assembly.GetExecutingAssembly().GetName().Version?.ToString() ?? "dev");

otelBuilder.WithTracing(tracing =>
{
    tracing
        .AddAspNetCoreInstrumentation(options => options.RecordException = true)
        .AddHttpClientInstrumentation()
        .AddSource("Croniq.Core")
        .AddSource("Croniq.Webhooks.Ingress");
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

app.UseCroniqWebhooks();

app.Run();
