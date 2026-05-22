using System.Text.Json.Serialization;

namespace Croniq.Runner.Sdk.Protocol;

internal sealed record PollRequest(
    [property: JsonPropertyName("runner_id")] string RunnerId,
    [property: JsonPropertyName("capabilities")] IReadOnlyList<string> Capabilities,
    [property: JsonPropertyName("max_inflight")] int MaxInflight,
    [property: JsonPropertyName("inflight")] IReadOnlyList<string> Inflight,
    [property: JsonPropertyName("instance_id"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? InstanceId,
    [property: JsonPropertyName("tags")] IReadOnlyList<string> Tags);
