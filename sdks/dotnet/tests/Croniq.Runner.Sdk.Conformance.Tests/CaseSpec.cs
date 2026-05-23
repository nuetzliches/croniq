namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// In-memory representation of a single conformance case YAML file.
/// All fields use snake_case to mirror the YAML/openapi.yaml convention.
/// </summary>
public sealed class CaseSpec
{
    public string Name { get; set; } = "";
    public string? Description { get; set; }
    public RunnerConfigSpec RunnerConfig { get; set; } = new();
    public List<HandlerSpec> Handlers { get; set; } = [];
    public List<ScriptEntrySpec> ServerScript { get; set; } = [];
    public ExpectationsSpec Expectations { get; set; } = new();

    /// <summary>
    /// Binding-specific directive: cancel the runner this many ms after
    /// RunAsync starts. If absent, the binding lets the runner poll until
    /// expectations are met or duration_max_ms elapses.
    /// </summary>
    public int? ShutdownAfterMs { get; set; }
}

public sealed class RunnerConfigSpec
{
    public string? RunnerId { get; set; }
    public string? RunnerIdPrefix { get; set; }
    public List<string> Capabilities { get; set; } = [];
    public List<string> Tags { get; set; } = [];
    public int? MaxInflight { get; set; }
    public string? ApiKey { get; set; }
    public string? BearerToken { get; set; }
    public int? PollTimeoutMs { get; set; }
    public int? RenewIntervalMs { get; set; }
    public int? DrainTimeoutMs { get; set; }
    public int? PollRetryDelayMs { get; set; }
    public int? CapacityBackoffMs { get; set; }
}

public sealed class HandlerSpec
{
    public string JobKey { get; set; } = "";
    public bool IsDefault { get; set; }
    public string? Schedule { get; set; }
    public string Behavior { get; set; } = "noop";
    public string? ErrorMessage { get; set; }
    public int? DurationMs { get; set; }
    public string? Level { get; set; }
    public string? Message { get; set; }
    public int? Count { get; set; }
    public int? IntervalMs { get; set; }
}

public sealed class ScriptEntrySpec
{
    /// <summary>e.g. "POST /v1/work/poll" — split into method + path on load.</summary>
    public string On { get; set; } = "";
    public int? MatchCount { get; set; }
    public RespondSpec Respond { get; set; } = new();

    public string Method => On.Split(' ', 2)[0];
    public string Path => On.Split(' ', 2)[1];
}

public sealed class RespondSpec
{
    public int Status { get; set; } = 200;
    public object? Body { get; set; }
    public int? DelayMs { get; set; }
    public Dictionary<string, string> Headers { get; set; } = new(StringComparer.OrdinalIgnoreCase);
}

public sealed class ExpectationsSpec
{
    public int? DurationMaxMs { get; set; }
    public List<HttpExpectation> Http { get; set; } = [];
}

public sealed class HttpExpectation
{
    public string Method { get; set; } = "GET";
    public string Path { get; set; } = "/";
    public int? ExactCount { get; set; }
    public int? MinCount { get; set; }
    public int? MaxCount { get; set; }
    public Dictionary<string, string> Headers { get; set; } = new(StringComparer.OrdinalIgnoreCase);
    public object? BodyMatch { get; set; }
}
