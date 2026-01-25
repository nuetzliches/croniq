using Croniq.Core;
using Croniq.Core.Hosting;
using Croniq.Core.Execution;
using Croniq.Hosting;
using Croniq.Options;
using Croniq.Rpc;
using Croniq.Sample.Jobs;
using Croniq.Webhooks;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;

var builder = Host.CreateApplicationBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.AddCroniqWorkerServices(builder.Configuration);
builder.Services.AddCroniqWebhookServices(builder.Configuration, includePlatformServices: false);
builder.Services.AddCroniqWebhookRateLimiter();
builder.Services.AddCroniqSampleJobs();

var dispatchOptions = builder.Configuration.GetSection("Croniq:WorkerDispatch").Get<WorkerDispatchOptions>() ?? new WorkerDispatchOptions();
if (dispatchOptions.EnableGrpc)
{
    if (string.IsNullOrWhiteSpace(dispatchOptions.GrpcEndpoint))
    {
        throw new InvalidOperationException("Croniq:WorkerDispatch:GrpcEndpoint is required when EnableGrpc is true.");
    }

    builder.Services.AddCroniqWorkerClient(options =>
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
    "Croniq.Worker",
    options => options.ConsoleLogFormat = "text");

builder.Services.AddCroniqFileExecutionLogStore(options =>
{
    builder.Configuration.GetSection("Croniq:Logging:Execution").Bind(options);
});
builder.Services.Configure<ExecutionLogRetentionOptions>(builder.Configuration.GetSection("Croniq:Logging:Execution:Retention"));
builder.Services.AddHostedService<ExecutionLogRetentionService>();

await builder.Build().RunAsync();
