using System;

namespace Croniq.Options;

public sealed class WorkerDispatchOptions
{
    /// <summary>
    /// Enables gRPC-based worker dispatch. When false, workers use polling.
    /// </summary>
    public bool EnableGrpc { get; set; }

    /// <summary>
    /// gRPC endpoint for Runner.Connect (for example: http://localhost:5001).
    /// Required when EnableGrpc is true.
    /// </summary>
    public string? GrpcEndpoint { get; set; }

    /// <summary>
    /// API key used for Runner.Connect authentication.
    /// </summary>
    public string? ApiKey { get; set; }

    /// <summary>
    /// Runner identity used for Runner.Connect. Defaults to Croniq:Core:InstanceId.
    /// </summary>
    public string? RunnerId { get; set; }

    /// <summary>
    /// Maximum in-flight assignments for the gRPC stream (0 = use WorkerHostOptions.BatchSize).
    /// </summary>
    public int MaxInflight { get; set; }

    /// <summary>
    /// Whether this runner accepts test executions.
    /// </summary>
    public bool AllowTestExecutions { get; set; }

    /// <summary>
    /// Optional capability tags for the runner.
    /// </summary>
    public string[]? Capabilities { get; set; }

    /// <summary>
    /// If true, fall back to polling when gRPC is unavailable.
    /// </summary>
    public bool EnablePollingFallback { get; set; } = true;

    /// <summary>
    /// Delay between reconnect attempts when gRPC disconnects.
    /// </summary>
    public TimeSpan ReconnectDelay { get; set; } = TimeSpan.FromSeconds(5);

    /// <summary>
    /// Polling delay when no work was processed during fallback.
    /// </summary>
    public TimeSpan FallbackIdleDelay { get; set; } = TimeSpan.FromSeconds(5);

    /// <summary>
    /// Polling delay when work was processed during fallback.
    /// </summary>
    public TimeSpan FallbackBusyDelay { get; set; } = TimeSpan.FromSeconds(1);

    /// <summary>
    /// Delay after a fallback polling error.
    /// </summary>
    public TimeSpan FallbackErrorDelay { get; set; } = TimeSpan.FromSeconds(5);
}
