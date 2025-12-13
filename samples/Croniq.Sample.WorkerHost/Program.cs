using System.Reflection;
using Croniq.Core;
using Croniq.Core.Options;
using Croniq.Core.Policies;
using Croniq.JobStore.InMemory;
using Croniq.Providers.Default;
using Croniq.Persistence.SqlServer;
using Croniq.Sample.Jobs;
using Croniq.Sample.WorkerHost;
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

builder.Services.Configure<CroniqOptions>(builder.Configuration.GetSection("Croniq:Core"));
builder.Services.Configure<MisfirePolicyOptions>(builder.Configuration.GetSection("Croniq:Policies:Misfire"));
builder.Services.Configure<ExecutionPolicyOptions>(builder.Configuration.GetSection("Croniq:Policies:Execution"));
builder.Services.Configure<PolicyOverrideOptions>(builder.Configuration.GetSection("Croniq:Policies:Overrides"));

builder.Services.AddCroniqDefaultProviders();
builder.Services.AddCroniqCore();
builder.Services.AddCroniqSampleJobs();

builder.Services.AddCroniqSamplePersistence(builder.Configuration);

builder.Services.AddCroniqObservability(
    builder.Configuration,
    builder.Logging,
    "Croniq.Worker");

builder.Services.AddCroniqWorkerHost();
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
