using Croniq.Core;
using Croniq.Core.Execution;
using Croniq.Hosting;
using Croniq.Sample.Jobs;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using OpenTelemetry.Metrics;
using OpenTelemetry.Trace;

var builder = Host.CreateApplicationBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.AddCroniqWorkerServices(builder.Configuration);
builder.Services.AddCroniqSampleJobs();

builder.Services.AddCroniqObservability(
    builder.Configuration,
    builder.Logging,
    "Croniq.Worker");

builder.Services.AddCroniqFileExecutionLogStore();
builder.Services.Configure<ExecutionLogRetentionOptions>(builder.Configuration.GetSection("Croniq:Logging:Execution:Retention"));
builder.Services.AddHostedService<ExecutionLogRetentionService>();
builder.Services.AddLogging(logging =>
{
    logging.SetMinimumLevel(LogLevel.Information);
    logging.AddCroniqExecutionLogSink();
    logging.AddSimpleConsole(options =>
    {
        options.SingleLine = true;
        options.TimestampFormat = "HH:mm:ss ";
    });
});

await builder.Build().RunAsync();
