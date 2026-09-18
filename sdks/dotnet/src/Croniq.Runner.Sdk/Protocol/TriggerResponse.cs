using System.Text.Json.Serialization;

namespace Croniq.Runner.Sdk.Protocol;

/// <summary>
/// Wire response of <c>POST /v1/trigger</c>. <c>deduplicated</c> is sent by
/// servers that support trigger idempotency keys and <c>coalesced</c> by
/// servers that support the <c>coalesce</c> job directive; older servers omit
/// either and the field defaults to <c>false</c>.
/// </summary>
internal sealed record TriggerResponse(
    [property: JsonPropertyName("execution_id")] string ExecutionId,
    [property: JsonPropertyName("queued")] int Queued,
    [property: JsonPropertyName("deduplicated")] bool Deduplicated = false,
    [property: JsonPropertyName("coalesced")] bool Coalesced = false);
