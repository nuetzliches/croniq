using System.Reflection;
using Croniq.Core;
using Croniq.Core.Options;
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

builder.Services.AddCroniqDefaultProviders();
builder.Services.AddCroniqCore();
builder.Services.AddCroniqSampleJobs();

ConfigurePersistence(builder);

var otelBuilder = builder.Services.AddCroniqObservability(
    builder.Configuration,
    builder.Logging,
    "Croniq.Worker",
    options => options.ServiceVersion = Assembly.GetExecutingAssembly().GetName().Version?.ToString() ?? "dev");

otelBuilder.WithTracing(tracing =>
{
    tracing
        .AddSource("Croniq.Core")
        .AddHttpClientInstrumentation();
});

otelBuilder.WithMetrics(metrics =>
{
    metrics
        .AddRuntimeInstrumentation()
        .AddHttpClientInstrumentation()
        .AddMeter("Croniq.Core");
});

builder.Services.AddHostedService<CroniqWorkerHostedService>();
builder.Services.AddLogging(logging =>
{
    logging.SetMinimumLevel(LogLevel.Information);
    logging.AddSimpleConsole(options =>
    {
        options.SingleLine = true;
        options.TimestampFormat = "HH:mm:ss ";
    });
});

await builder.Build().RunAsync();

static void ConfigurePersistence(HostApplicationBuilder builder)
{
    var mode = builder.Configuration["Croniq:Persistence:Mode"] ?? "InMemory";
    if (string.Equals(mode, "SqlServer", StringComparison.OrdinalIgnoreCase))
    {
        var sqlSection = builder.Configuration.GetSection("Croniq:SqlServer");
        var connection = sqlSection["ConnectionString"];
        if (string.IsNullOrWhiteSpace(connection))
        {
            throw new InvalidOperationException("Croniq:SqlServer:ConnectionString is required when Persistence.Mode = SqlServer.");
        }

        var persistenceSection = builder.Configuration.GetSection("Croniq:Persistence:SqlServer");
        builder.Services.AddCroniqSqlServerPersistence(options =>
        {
            sqlSection.Bind(options);
            options.ConnectionString = connection;
        }, persistenceSection.Exists() ? persistence => persistenceSection.Bind(persistence) : null);
    }
    else
    {
        builder.Services.AddCroniqInMemoryJobStore();
    }
}
