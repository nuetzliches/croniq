using System.Text.Json;
using System.Text.Json.Serialization;

namespace Croniq.Runner.Sdk.Protocol;

/// <summary>
/// One unit of work returned by <c>POST /v1/work/poll</c>. Maps 1:1 to
/// the Rust SDK's <c>WorkAssignment</c> struct.
/// </summary>
internal sealed record WorkAssignment(
    [property: JsonPropertyName("execution_id")] string ExecutionId,
    [property: JsonPropertyName("job_key")] string JobKey,
    [property: JsonPropertyName("fire_at")] string FireAt,
    [property: JsonPropertyName("attempt")] int Attempt,
    [property: JsonPropertyName("metadata")] JsonElement Metadata,
    [property: JsonPropertyName("timeout")] string Timeout,
    // Original logical fire time (RFC 3339); null when the server predates the
    // field. Trailing + defaulted to keep positional construction compatible.
    [property: JsonPropertyName("scheduled_for")] string? ScheduledFor = null);
