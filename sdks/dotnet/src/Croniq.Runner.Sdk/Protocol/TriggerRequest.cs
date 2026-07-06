using System.Text.Json.Serialization;

namespace Croniq.Runner.Sdk.Protocol;

internal sealed record TriggerRequest(
    [property: JsonPropertyName("job_key")] string JobKey,
    [property: JsonPropertyName("metadata"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] IReadOnlyDictionary<string, string>? Metadata,
    [property: JsonPropertyName("require"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] IReadOnlyList<string>? Require,
    [property: JsonPropertyName("prefer"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] IReadOnlyList<string>? Prefer,
    [property: JsonPropertyName("timeout"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Timeout,
    [property: JsonPropertyName("idempotency_key"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? IdempotencyKey = null);
