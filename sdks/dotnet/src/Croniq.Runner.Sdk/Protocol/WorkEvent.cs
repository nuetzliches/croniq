using System.Text.Json.Serialization;

namespace Croniq.Runner.Sdk.Protocol;

/// <summary>
/// A structured log event pushed to a Croniq execution via
/// <c>POST /v1/work/{execution_id}/events</c>. Callers construct these
/// values (or use the helpers on <see cref="CroniqExecutionContext"/>); the SDK
/// auto-enriches the <see cref="Fields"/> map with <c>job_key</c>,
/// <c>runner_id</c>, and (when set) <c>runner_tags</c> before sending.
/// </summary>
public sealed record WorkEvent
{
    /// <summary>
    /// Severity level. Free-form string matching the server's accepted set
    /// (typically "trace", "debug", "info", "warn", "error"). When <c>null</c>,
    /// the server applies its default.
    /// </summary>
    [JsonPropertyName("level")]
    public string? Level { get; init; }

    /// <summary>Human-readable event text.</summary>
    [JsonPropertyName("message")]
    public required string Message { get; init; }

    /// <summary>
    /// Structured fields attached to the event. Existing keys win over
    /// SDK-injected ones — explicit caller values are preserved.
    /// </summary>
    [JsonPropertyName("fields")]
    public IReadOnlyDictionary<string, string>? Fields { get; init; }
}
