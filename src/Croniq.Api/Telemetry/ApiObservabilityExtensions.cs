using System;
using Croniq.Core;
using Croniq.Options;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using OpenTelemetry;
using OpenTelemetry.Metrics;
using OpenTelemetry.Trace;

namespace Croniq.Api;

public static class ApiObservabilityExtensions
{
    public static OpenTelemetryBuilder AddCroniqApiObservability(
        this IServiceCollection services,
        IConfiguration configuration,
        ILoggingBuilder loggingBuilder,
        Action<CroniqObservabilityOptions>? configure = null,
        OpenTelemetryBuilder? builder = null,
        Action<TracerProviderBuilder>? configureTracing = null,
        Action<MeterProviderBuilder>? configureMetrics = null)
    {
        ArgumentNullException.ThrowIfNull(services);
        ArgumentNullException.ThrowIfNull(configuration);
        ArgumentNullException.ThrowIfNull(loggingBuilder);

        var otelBuilder = builder ?? services.AddCroniqObservability(configuration, loggingBuilder, "Croniq.Api", configure);

        otelBuilder.WithTracing(tracing =>
        {
            tracing
                .AddAspNetCoreInstrumentation(options => options.RecordException = true)
                .AddHttpClientInstrumentation()
                .AddSource("Croniq.Core")
                .AddSource("Croniq.Api.Trigger")
                .AddSource("Croniq.Api.Grpc.WebhookIngress");

            configureTracing?.Invoke(tracing);
        });

        otelBuilder.WithMetrics(metrics =>
        {
            metrics
                .AddAspNetCoreInstrumentation()
                .AddHttpClientInstrumentation()
                .AddRuntimeInstrumentation()
                .AddMeter("Croniq.Core")
                .AddMeter("Croniq.Api");

            configureMetrics?.Invoke(metrics);
        });

        return otelBuilder;
    }
}
