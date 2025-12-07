using System.Collections.Generic;
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
using OpenTelemetry.Exporter;
using OpenTelemetry.Logs;
using OpenTelemetry.Metrics;
using OpenTelemetry.Resources;
using OpenTelemetry.Trace;

// Enable OTLP gRPC export over plaintext HTTP/2 inside the devstack network.
AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);

var builder = Host.CreateApplicationBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.Configure<CroniqOptions>(builder.Configuration.GetSection("Croniq:Core"));

builder.Services.AddCroniqDefaultProviders();
builder.Services.AddCroniqCore();
builder.Services.AddCroniqSampleJobs();

ConfigurePersistence(builder);
ConfigureObservability(builder);

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

static void ConfigureObservability(HostApplicationBuilder builder)
{
    var otlpEndpoint = builder.Configuration["Croniq:Observability:OtlpEndpoint"] ?? "http://otel-collector:4317";
    var otlpProtocolValue = builder.Configuration["Croniq:Observability:OtlpProtocol"];
    var otlpProtocol = string.Equals(otlpProtocolValue, "grpc", StringComparison.OrdinalIgnoreCase)
        ? OtlpExportProtocol.Grpc
        : OtlpExportProtocol.HttpProtobuf;
    var serviceVersion = Assembly.GetExecutingAssembly().GetName().Version?.ToString() ?? "dev";
    var environment = builder.Configuration["Croniq:Core:EnvironmentTag"] ?? "dev";
    var tenantId = builder.Configuration["Croniq:Core:TenantId"] ?? "default";
    var otlpTracesEndpoint = ResolveOtlpEndpoint(otlpEndpoint, otlpProtocol, "traces");
    var otlpMetricsEndpoint = ResolveOtlpEndpoint(otlpEndpoint, otlpProtocol, "metrics");
    var otlpLogsEndpoint = ResolveOtlpEndpoint(otlpEndpoint, otlpProtocol, "logs");
    builder.Services.AddOpenTelemetry()
        .ConfigureResource(resource =>
        {
            resource.AddService("Croniq.Worker", serviceVersion: serviceVersion);
            resource.AddAttributes(new[]
            {
                new KeyValuePair<string, object>("deployment.environment", environment),
                new KeyValuePair<string, object>("croniq.tenant_id", tenantId)
            });
        })
        .WithTracing(tracing =>
        {
            tracing
                .AddSource("Croniq.Core")
                .AddHttpClientInstrumentation()
                .AddOtlpExporter(options =>
                {
                    options.Endpoint = otlpTracesEndpoint;
                    options.Protocol = otlpProtocol;
                });
        })
        .WithMetrics(metrics =>
        {
            metrics
                .AddRuntimeInstrumentation()
                .AddHttpClientInstrumentation()
                .AddMeter("Croniq.Core")
                .AddOtlpExporter(options =>
                {
                    options.Endpoint = otlpMetricsEndpoint;
                    options.Protocol = otlpProtocol;
                });
        });

    builder.Logging.AddOpenTelemetry(logging =>
    {
        logging.IncludeFormattedMessage = true;
        logging.IncludeScopes = true;
        logging.ParseStateValues = true;
        logging.AddOtlpExporter(options =>
        {
            options.Endpoint = otlpLogsEndpoint;
            options.Protocol = otlpProtocol;
        });
    });
}

static Uri ResolveOtlpEndpoint(string endpoint, OtlpExportProtocol protocol, string signal)
{
    if (protocol == OtlpExportProtocol.HttpProtobuf)
    {
        var trimmed = endpoint.TrimEnd('/');
        return new Uri($"{trimmed}/v1/{signal}");
    }

    return new Uri(endpoint);
}
