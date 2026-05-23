using System.Text.Json.Serialization;

namespace Croniq.Runner.Sdk.Protocol;

internal sealed record RenewRequest(
    [property: JsonPropertyName("runner_id")] string RunnerId,
    [property: JsonPropertyName("execution_id")] string ExecutionId);
