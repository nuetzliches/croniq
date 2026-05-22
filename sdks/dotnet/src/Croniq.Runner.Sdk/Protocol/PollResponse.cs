using System.Text.Json.Serialization;

namespace Croniq.Runner.Sdk.Protocol;

internal sealed record PollResponse(
    [property: JsonPropertyName("work")] IReadOnlyList<WorkAssignment> Work,
    [property: JsonPropertyName("cancel")] IReadOnlyList<string> Cancel);
