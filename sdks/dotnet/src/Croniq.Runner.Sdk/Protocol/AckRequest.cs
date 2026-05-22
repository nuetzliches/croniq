using System.Text.Json.Serialization;

namespace Croniq.Runner.Sdk.Protocol;

internal sealed record AckRequest(
    [property: JsonPropertyName("runner_id")] string RunnerId,
    [property: JsonPropertyName("execution_id")] string ExecutionId,
    [property: JsonPropertyName("status")] string Status,
    [property: JsonPropertyName("error"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Error,
    [property: JsonPropertyName("duration_ms"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] long? DurationMs,
    [property: JsonPropertyName("attempt")] int Attempt);
