using System.Net;
using Croniq.Api;
using Croniq.Hosting;
using Croniq.Persistence.Abstractions;
using Croniq.Webhooks;
using Croniq.Webhooks.Options;
using Microsoft.AspNetCore.HttpOverrides;
using Microsoft.Extensions.Options;
using BclIpNetwork = System.Net.IPNetwork;

var builder = WebApplication.CreateBuilder(args);
builder.AddCroniqHostDefaults();

builder.Services.AddEndpointsApiExplorer();
builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqApiRateLimiter();
builder.Services.AddCroniqApiCors(builder.Configuration, builder.Environment);
builder.Services.AddCroniqApiSchemas();

builder.Services.AddCroniqWebhookServices(builder.Configuration, includePlatformServices: false);
builder.Services.AddCroniqWebhookRateLimiter();

builder.Services.AddCroniqApiObservability(builder.Configuration, builder.Logging);

builder.Services.AddCroniqJobsFromConfiguration(builder.Configuration);

var forwardedHeaders = builder.Configuration
    .GetSection("Croniq:Api:ForwardedHeaders")
    .Get<CroniqForwardedHeadersOptions>() ?? new CroniqForwardedHeadersOptions();

var app = builder.Build();

if (forwardedHeaders.Enabled)
{
    var options = new ForwardedHeadersOptions
    {
        ForwardedHeaders = ForwardedHeaders.XForwardedFor | ForwardedHeaders.XForwardedProto,
        ForwardLimit = Math.Max(1, forwardedHeaders.ForwardLimit)
    };

    var hasKnownProxy = false;
    foreach (var cidr in forwardedHeaders.KnownNetworks)
    {
        if (string.IsNullOrWhiteSpace(cidr))
        {
            continue;
        }

        if (BclIpNetwork.TryParse(cidr, out var network))
        {
            options.KnownIPNetworks.Add(network);
            hasKnownProxy = true;
        }
        else
        {
            app.Logger.LogWarning("Croniq:Api:ForwardedHeaders:KnownNetworks contains invalid CIDR '{Cidr}'.", cidr);
        }
    }

    foreach (var proxy in forwardedHeaders.KnownProxies)
    {
        if (string.IsNullOrWhiteSpace(proxy))
        {
            continue;
        }

        if (IPAddress.TryParse(proxy, out var address))
        {
            options.KnownProxies.Add(address);
            hasKnownProxy = true;
        }
        else
        {
            app.Logger.LogWarning("Croniq:Api:ForwardedHeaders:KnownProxies contains invalid IP '{Proxy}'.", proxy);
        }
    }

    if (!hasKnownProxy)
    {
        app.Logger.LogWarning(
            "Croniq:Api:ForwardedHeaders is enabled but no KnownNetworks/Proxies were configured; forwarded headers will only be accepted from loopback.");
    }

    app.UseForwardedHeaders(options);
}

app.UseCroniqApiSwaggerUi(builder.Configuration);
app.UseCroniqApiCors();
app.UseCroniqApi();

app.MapCroniqSchedulerGrpc();
app.MapCroniqWorkerGrpc();
app.MapCroniqWebhookActivityGrpc();

var webhookOptions = app.Services.GetRequiredService<IOptions<CroniqWebhookOptions>>().Value;
if (webhookOptions.Ingress.DispatchMode == WebhookIngressDispatchMode.StoreOnly
    && app.Services.GetService<IWebhookIngressEventStore>() is not null)
{
    app.MapCroniqWebhookIngressGrpc();
}

app.Run();
