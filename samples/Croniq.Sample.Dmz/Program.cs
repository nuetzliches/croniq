using Croniq.Api;
using Croniq.Webhooks;

var builder = WebApplication.CreateBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqApiRateLimiter();

builder.Services.AddCroniqWebhookServices(builder.Configuration);
builder.Services.AddCroniqWebhookRateLimiter();

var otelBuilder = builder.Services.AddCroniqApiObservability(
    builder.Configuration,
    builder.Logging);

builder.Services.AddCroniqWebhookObservability(
    builder.Configuration,
    builder.Logging,
    builder: otelBuilder);

var app = builder.Build();

app.UseCroniqApi();
app.MapCroniqWebhookIngressGrpc();
app.UseCroniqWebhooks(mapHealthEndpoints: false);

app.Run();
