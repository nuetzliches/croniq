using System;
using System.Collections.Generic;
using System.Reflection;
using Croniq.Options;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using OpenTelemetry;
using OpenTelemetry.Exporter;
using OpenTelemetry.Metrics;
using OpenTelemetry.Resources;
using OpenTelemetry.Trace;
using Serilog;
using Serilog.Enrichers.Span;
using Serilog.Formatting.Json;
using Serilog.Sinks.OpenTelemetry;

namespace Croniq.Core;

public static class CroniqObservabilityExtensions
{
    private const string CoreSectionPath = "Croniq:Core";
    private const string ObservabilitySectionPath = "Croniq:Observability";

    public static OpenTelemetryBuilder AddCroniqObservability(
        this IServiceCollection services,
        IConfiguration configuration,
        ILoggingBuilder loggingBuilder,
        string serviceName,
        Action<CroniqObservabilityOptions>? configure = null)
    {
        ArgumentNullException.ThrowIfNull(services);
        ArgumentNullException.ThrowIfNull(configuration);
        if (string.IsNullOrWhiteSpace(serviceName))
        {
            throw new ArgumentException("Service name is required.", nameof(serviceName));
        }

        ArgumentNullException.ThrowIfNull(loggingBuilder);

        var options = BuildOptions(configuration, serviceName, configure);
        var protocol = ParseProtocol(options.OtlpProtocol);
        var otlpEndpoint = options.OtlpEndpoint;

        if (options.EnableHttp2UnencryptedSupport && protocol == OtlpExportProtocol.Grpc && IsHttpEndpoint(otlpEndpoint))
        {
            AppContext.SetSwitch("System.Net.Http.SocketsHttpHandler.Http2UnencryptedSupport", true);
        }

        var builder = services.AddOpenTelemetry();

        var resourceAttributes = BuildResourceAttributes(options);

        builder.ConfigureResource(resource =>
        {
            resource.AddService(
                options.ServiceName,
                serviceVersion: options.ServiceVersion,
                serviceInstanceId: options.ServiceInstanceId);

            resource.AddAttributes(resourceAttributes);
        });

        if (options.EnableTracing)
        {
            builder.WithTracing(tracing =>
            {
                tracing.AddOtlpExporter(exporter =>
                {
                    exporter.Protocol = protocol;
                    exporter.Endpoint = ResolveSignalEndpoint(otlpEndpoint, protocol, "traces");
                });
            });
        }

        if (options.EnableMetrics)
        {
            builder.WithMetrics(metrics =>
            {
                metrics.AddOtlpExporter(exporter =>
                {
                    exporter.Protocol = protocol;
                    exporter.Endpoint = ResolveSignalEndpoint(otlpEndpoint, protocol, "metrics");
                });
            });
        }

        if (options.EnableLogging)
        {
            ConfigureSerilogLogging(loggingBuilder, options, protocol, otlpEndpoint, resourceAttributes);
        }

        return builder;
    }

    private static CroniqObservabilityOptions BuildOptions(
        IConfiguration configuration,
        string serviceName,
        Action<CroniqObservabilityOptions>? configure)
    {
        var options = new CroniqObservabilityOptions();
        configuration.GetSection(ObservabilitySectionPath).Bind(options);

        options.ServiceName = serviceName;
        options.Environment = configuration[$"{CoreSectionPath}:EnvironmentTag"] ?? options.Environment;
        options.TenantId = configuration[$"{CoreSectionPath}:TenantReference"] ?? options.TenantId;
        options.ServiceInstanceId ??= configuration[$"{CoreSectionPath}:InstanceId"];
        options.ServiceVersion ??= Assembly.GetEntryAssembly()?.GetName().Version?.ToString() ?? "dev";
        options.OtlpEndpoint ??= "http://otel-collector:4317";
        options.OtlpProtocol ??= "grpc";

        configure?.Invoke(options);

        options.ServiceInstanceId ??= options.ServiceName;
        ApplyDefaultMinimumLevelOverrides(options.MinimumLevelOverrides);
        return options;
    }

    private static void ApplyDefaultMinimumLevelOverrides(IDictionary<string, Serilog.Events.LogEventLevel> overrides)
    {
        static void Ensure(IDictionary<string, Serilog.Events.LogEventLevel> target, string key, Serilog.Events.LogEventLevel level)
        {
            if (!target.ContainsKey(key))
            {
                target[key] = level;
            }
        }

        Ensure(overrides, "Microsoft.EntityFrameworkCore.Database.Command", Serilog.Events.LogEventLevel.Warning);
        Ensure(overrides, "Microsoft.Hosting.Lifetime", Serilog.Events.LogEventLevel.Warning);
        Ensure(overrides, "Microsoft.AspNetCore.Hosting.Diagnostics", Serilog.Events.LogEventLevel.Warning);
        Ensure(overrides, "Microsoft.AspNetCore.Mvc.Infrastructure.DefaultActionDescriptorCollectionProvider", Serilog.Events.LogEventLevel.Warning);
    }

    private static IReadOnlyList<KeyValuePair<string, object>> BuildResourceAttributes(CroniqObservabilityOptions options)
    {
        var attributes = new List<KeyValuePair<string, object>>
        {
            new("deployment.environment", options.Environment),
            new("croniq.tenant_id", options.TenantId)
        };

        if (!string.IsNullOrWhiteSpace(options.ServiceInstanceId))
        {
            attributes.Add(new KeyValuePair<string, object>("service.instance.id", options.ServiceInstanceId));
        }

        foreach (var attribute in options.ResourceAttributes)
        {
            if (string.IsNullOrWhiteSpace(attribute.Key) || string.IsNullOrWhiteSpace(attribute.Value))
            {
                continue;
            }

            attributes.Add(new KeyValuePair<string, object>(attribute.Key, attribute.Value));
        }

        return attributes;
    }

    private static OtlpExportProtocol ParseProtocol(string? value)
    {
        return string.Equals(value, "grpc", StringComparison.OrdinalIgnoreCase)
            ? OtlpExportProtocol.Grpc
            : OtlpExportProtocol.HttpProtobuf;
    }

    private static Uri ResolveSignalEndpoint(string endpoint, OtlpExportProtocol protocol, string signal)
    {
        if (protocol == OtlpExportProtocol.HttpProtobuf)
        {
            var trimmed = endpoint.TrimEnd('/');
            return new Uri($"{trimmed}/v1/{signal}");
        }

        return new Uri(endpoint);
    }

    private static bool IsHttpEndpoint(string endpoint)
    {
        return Uri.TryCreate(endpoint, UriKind.Absolute, out var uri) &&
               string.Equals(uri.Scheme, Uri.UriSchemeHttp, StringComparison.OrdinalIgnoreCase);
    }

    private static void ConfigureSerilogLogging(
        ILoggingBuilder loggingBuilder,
        CroniqObservabilityOptions options,
        OtlpExportProtocol protocol,
        string otlpEndpoint,
        IReadOnlyList<KeyValuePair<string, object>> resourceAttributes)
    {
        loggingBuilder.ClearProviders();

        var loggerConfiguration = new LoggerConfiguration()
            .MinimumLevel.Is(options.MinimumLogLevel)
            .Enrich.FromLogContext()
            .Enrich.WithSpan()
            .Enrich.WithProperty("service.name", options.ServiceName);

        foreach (var overridePair in options.MinimumLevelOverrides)
        {
            loggerConfiguration.MinimumLevel.Override(overridePair.Key, overridePair.Value);
        }

        if (!string.IsNullOrWhiteSpace(options.ServiceInstanceId))
        {
            loggerConfiguration.Enrich.WithProperty("service.instance.id", options.ServiceInstanceId);
        }

        loggerConfiguration.Enrich.WithProperty("deployment.environment", options.Environment);
        loggerConfiguration.Enrich.WithProperty("croniq.tenant_id", options.TenantId);

        foreach (var attribute in resourceAttributes)
        {
            loggerConfiguration.Enrich.WithProperty(attribute.Key, attribute.Value);
        }

        if (options.EnableConsoleLogging)
        {
            if (string.Equals(options.ConsoleLogFormat, "text", StringComparison.OrdinalIgnoreCase))
            {
                loggerConfiguration.WriteTo.Console();
            }
            else
            {
                loggerConfiguration.WriteTo.Console(new JsonFormatter());
            }
        }

        if (options.EnableOtlpLogExport)
        {
            loggerConfiguration.WriteTo.OpenTelemetry(exporter =>
            {
                exporter.Endpoint = ResolveSignalEndpoint(otlpEndpoint, protocol, "logs").ToString();
                exporter.Protocol = protocol == OtlpExportProtocol.Grpc
                    ? OtlpProtocol.Grpc
                    : OtlpProtocol.HttpProtobuf;

                foreach (var attribute in resourceAttributes)
                {
                    exporter.ResourceAttributes[attribute.Key] = attribute.Value;
                }
            });
        }

        var serilogLogger = loggerConfiguration.CreateLogger();
        loggingBuilder.AddSerilog(serilogLogger, dispose: true);
    }
}
