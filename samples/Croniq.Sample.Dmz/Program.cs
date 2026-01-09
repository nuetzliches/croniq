using Croniq.Api;
using Croniq.Webhooks;
using Microsoft.AspNetCore.Server.Kestrel.Core;

var builder = WebApplication.CreateBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.WebHost.ConfigureKestrel(options =>
{
    options.ConfigureEndpointDefaults(endpoint =>
    {
        endpoint.Protocols = HttpProtocols.Http1AndHttp2;
    });
});

builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqApiRateLimiter();

builder.Services.AddCroniqWebhookServices(builder.Configuration, includePlatformServices: false);
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
