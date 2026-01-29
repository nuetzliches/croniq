using Croniq.Core;
using Croniq.Core.Hosting;
using Croniq.Hosting;
using Croniq.Options;
using Croniq.Rpc;
using Croniq.Webhooks;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;

var builder = Host.CreateApplicationBuilder(args);

builder.Services.AddCroniqWorkerServices(builder.Configuration);
builder.Services.AddCroniqWebhookServices(builder.Configuration, includePlatformServices: false);

var dispatchOptions = builder.Configuration.GetSection("Croniq:WorkerDispatch").Get<WorkerDispatchOptions>() ?? new WorkerDispatchOptions();
if (dispatchOptions.EnableGrpc)
{
    if (string.IsNullOrWhiteSpace(dispatchOptions.GrpcEndpoint))
    {
        throw new InvalidOperationException("Croniq:WorkerDispatch:GrpcEndpoint is required when EnableGrpc is true.");
    }

    builder.Services.AddCroniqRunnerClient(options =>
    {
        options.Endpoint = dispatchOptions.GrpcEndpoint!.Trim();
        options.ApiKey = dispatchOptions.ApiKey;
    });
    builder.Services.AddSingleton<CroniqWorkerGrpcDispatchHostedService>();
    builder.Services.AddSingleton<IHostedService>(sp => sp.GetRequiredService<CroniqWorkerGrpcDispatchHostedService>());
    builder.Services.AddSingleton<IWorkerDispatchStatusProvider>(sp => sp.GetRequiredService<CroniqWorkerGrpcDispatchHostedService>());
}

builder.Services.AddCroniqObservability(
    builder.Configuration,
    builder.Logging,
    "Croniq.Worker");

builder.Services.AddCroniqJobsFromConfiguration(builder.Configuration);

var app = builder.Build();
var lifetime = app.Services.GetRequiredService<IHostApplicationLifetime>();
Console.CancelKeyPress += (_, args) =>
{
    args.Cancel = true;
    lifetime.StopApplication();
};

await app.RunAsync();
