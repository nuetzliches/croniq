using System.Text.Json;
using System.Text.Json.Serialization;

using Croniq.Runner.Sdk.Protocol;
using Croniq.Runner.Sdk.ShellExec;

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Source-generated <see cref="JsonSerializerContext"/> for all wire types.
/// Lets the SDK serialize/deserialize without runtime reflection — AOT- and
/// trim-friendly, and faster than reflection-based <see cref="JsonSerializer"/>.
/// </summary>
[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.SnakeCaseLower,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    WriteIndented = false)]
[JsonSerializable(typeof(PollRequest))]
[JsonSerializable(typeof(PollResponse))]
[JsonSerializable(typeof(WorkAssignment))]
[JsonSerializable(typeof(AckRequest))]
[JsonSerializable(typeof(RenewRequest))]
[JsonSerializable(typeof(RegisterJobRequest))]
[JsonSerializable(typeof(WorkEvent))]
[JsonSerializable(typeof(IReadOnlyList<WorkEvent>))]
[JsonSerializable(typeof(WorkEvent[]))]
[JsonSerializable(typeof(EventsResponse))]
[JsonSerializable(typeof(RegisterJobResponse))]
[JsonSerializable(typeof(RunnerExec))]
[JsonSerializable(typeof(TriggerRequest))]
[JsonSerializable(typeof(TriggerResponse))]
internal sealed partial class CroniqJsonContext : JsonSerializerContext
{
}

internal sealed record EventsResponse(
    [property: JsonPropertyName("accepted")] int Accepted);

internal sealed record RegisterJobResponse(
    [property: JsonPropertyName("job_key")] string JobKey,
    [property: JsonPropertyName("trigger_id")] string? TriggerId,
    [property: JsonPropertyName("status")] string? Status);
