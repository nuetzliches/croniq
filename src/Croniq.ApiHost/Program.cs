using Croniq.Api;
using Croniq.Hosting;
using Croniq.Persistence.Abstractions;
using Croniq.Webhooks;
using Croniq.Webhooks.Options;
using Microsoft.AspNetCore.Server.Kestrel.Core;
using Microsoft.Extensions.Options;

var builder = WebApplication.CreateBuilder(args);

builder.WebHost.ConfigureKestrel(options =>
{
    options.ConfigureEndpointDefaults(endpoint =>
    {
        endpoint.Protocols = HttpProtocols.Http1AndHttp2;
    });
});

builder.Services.AddEndpointsApiExplorer();
builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqApiRateLimiter();
builder.Services.AddCroniqApiSchemas();

builder.Services.AddCroniqWebhookServices(builder.Configuration, includePlatformServices: false);

builder.Services.AddCroniqApiObservability(builder.Configuration, builder.Logging);

builder.Services.AddCroniqJobsFromConfiguration(builder.Configuration);

var app = builder.Build();

app.UseCroniqApiSwaggerUi(builder.Configuration);
app.UseCroniqApi();

app.MapCroniqSchedulerGrpc();
app.MapCroniqWorkerGrpc();

var webhookOptions = app.Services.GetRequiredService<IOptions<CroniqWebhookOptions>>().Value;
if (webhookOptions.Ingress.DispatchMode == WebhookIngressDispatchMode.StoreOnly
    && app.Services.GetService<IWebhookIngressEventStore>() is not null)
{
    app.MapCroniqWebhookIngressGrpc();
}

app.Run();
