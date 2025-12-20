using System;
using System.Collections.Generic;
using Serilog.Events;

namespace Croniq.Options;

public sealed class CroniqObservabilityOptions
{
    /// <summary>
    /// Logical service name reported to OpenTelemetry. Defaults to "Croniq.App".
    /// </summary>
    public string ServiceName { get; set; } = "Croniq.App";

    /// <summary>
    /// Optional service version metadata. Defaults to the entry assembly version.
    /// </summary>
    public string? ServiceVersion { get; set; }

    /// <summary>
    /// Optional service instance identifier. Defaults to Croniq:Core:InstanceId when available.
    /// </summary>
    public string? ServiceInstanceId { get; set; }

    /// <summary>
    /// Croniq environment tag (e.g., dev, prod) forwarded as deployment.environment.
    /// </summary>
    public string Environment { get; set; } = "dev";

    /// <summary>
    /// Croniq tenant identifier forwarded via croniq.tenant_id.
    /// </summary>
    public string TenantId { get; set; } = "default";

    /// <summary>
    /// Base OTLP endpoint (without /v1/* suffix) the exporters should talk to.
    /// </summary>
    public string OtlpEndpoint { get; set; } = "http://otel-collector:4317";

    /// <summary>
    /// OTLP protocol ("grpc" or "http").
    /// </summary>
    public string OtlpProtocol { get; set; } = "grpc";

    /// <summary>
    /// Enables trace exporter registration when true (default).
    /// </summary>
    public bool EnableTracing { get; set; } = true;

    /// <summary>
    /// Enables metrics exporter registration when true (default).
    /// </summary>
    public bool EnableMetrics { get; set; } = true;

    /// <summary>
    /// Enables Serilog + log exporter registration when true (default).
    /// </summary>
    public bool EnableLogging { get; set; } = true;

    /// <summary>
    /// Enables JSON console logging via Serilog when true (default).
    /// </summary>
    public bool EnableConsoleLogging { get; set; } = true;

    /// <summary>
    /// Enables OTLP log export via Serilog sink when true (default).
    /// </summary>
    public bool EnableOtlpLogExport { get; set; } = true;

    /// <summary>
    /// Optional per-category minimum log level overrides (e.g., suppress verbose EF Core logs).
    /// </summary>
    public Dictionary<string, LogEventLevel> MinimumLevelOverrides { get; set; } = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Minimum log level applied to the Serilog pipeline.
    /// </summary>
    public LogEventLevel MinimumLogLevel { get; set; } = LogEventLevel.Information;

    /// <summary>
    /// Adds AppContext switch for HTTP/2 over plaintext when exporting via gRPC to http endpoints.
    /// </summary>
    public bool EnableHttp2UnencryptedSupport { get; set; } = true;

    /// <summary>
    /// Custom resource attributes merged into the OpenTelemetry resource builder.
    /// </summary>
    public Dictionary<string, string> ResourceAttributes { get; set; } = new(StringComparer.OrdinalIgnoreCase);
}
