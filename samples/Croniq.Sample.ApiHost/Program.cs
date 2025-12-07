using System.Collections.Generic;
using System.Reflection;
using Croniq.Api;
using Croniq.Sample.Jobs;
using OpenTelemetry.Exporter;
using OpenTelemetry.Logs;
using OpenTelemetry.Metrics;
using OpenTelemetry.Resources;
using OpenTelemetry.Trace;

// Enable OTLP gRPC export over plaintext HTTP/2 inside the devstack network.
AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);

var builder = WebApplication.CreateBuilder(args);

builder.Configuration
    .AddJsonFile("appsettings.Development.json", optional: true, reloadOnChange: true)
    .AddEnvironmentVariables();

builder.Services.AddCroniqApiServices(builder.Configuration);
builder.Services.AddCroniqSampleJobs();
builder.Services.AddCroniqApiRateLimiter();

ConfigureObservability(builder);

var app = builder.Build();

app.UseCroniqApi();

app.Run();

static void ConfigureObservability(WebApplicationBuilder builder)
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
            resource.AddService("Croniq.Api", serviceVersion: serviceVersion);
            resource.AddAttributes(new[]
            {
                new KeyValuePair<string, object>("deployment.environment", environment),
                new KeyValuePair<string, object>("croniq.tenant_id", tenantId)
            });
        })
        .WithTracing(tracing =>
        {
            tracing
                .AddAspNetCoreInstrumentation(options => options.RecordException = true)
                .AddHttpClientInstrumentation()
                .AddSource("Croniq.Core")
                .AddSource("Croniq.Api.Trigger")
                .AddOtlpExporter(options =>
                {
                    options.Endpoint = otlpTracesEndpoint;
                    options.Protocol = otlpProtocol;
                });
        })
        .WithMetrics(metrics =>
        {
            metrics
                .AddAspNetCoreInstrumentation()
                .AddHttpClientInstrumentation()
                .AddRuntimeInstrumentation()
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
