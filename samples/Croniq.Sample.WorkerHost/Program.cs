using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Hosting;
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
