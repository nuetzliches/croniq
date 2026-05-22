using System.Text.Json.Serialization;

namespace Croniq.Runner.Sdk.Protocol;

internal sealed record RegisterJobRequest(
    [property: JsonPropertyName("job_key")] string JobKey,
    [property: JsonPropertyName("schedule")] string Schedule,
    [property: JsonPropertyName("timezone"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Timezone,
    [property: JsonPropertyName("timeout"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Timeout,
    [property: JsonPropertyName("runner_id"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? RunnerId,
    [property: JsonPropertyName("capabilities")] IReadOnlyList<string> Capabilities,
    [property: JsonPropertyName("description"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Description);
