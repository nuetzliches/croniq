using System.Text.Json.Serialization;

namespace Croniq.Runner.Sdk.Protocol;

/// <remarks>
/// <c>Metadata</c> values are <c>object?</c>, not <c>string</c>: the server
/// forwards trigger metadata to the handler verbatim and explicitly does not
/// flatten or stringify it, so nested objects and non-string scalars have to
/// survive serialisation (issue #554). Every other SDK types this open too —
/// Go <c>map[string]any</c>, Python <c>dict[str, Any]</c>, TypeScript
/// <c>Record&lt;string, unknown&gt;</c>, Java <c>Map&lt;String, Object&gt;</c>.
/// </remarks>
internal sealed record TriggerRequest(
    [property: JsonPropertyName("job_key")] string JobKey,
    [property: JsonPropertyName("metadata"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] IReadOnlyDictionary<string, object?>? Metadata,
    [property: JsonPropertyName("require"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] IReadOnlyList<string>? Require,
    [property: JsonPropertyName("prefer"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] IReadOnlyList<string>? Prefer,
    [property: JsonPropertyName("timeout"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Timeout,
    [property: JsonPropertyName("idempotency_key"), JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? IdempotencyKey = null)
{
    /// <summary>
    /// Builds a request with explicitly <em>empty</em> optionals normalized to
    /// <c>null</c>, so <c>JsonIgnoreCondition.WhenWritingNull</c> omits them
    /// (issue #553).
    /// </summary>
    /// <remarks>
    /// A caller passing an empty collection or a blank string must produce the
    /// same wire body as one who passed nothing. The server already reads an
    /// empty <c>require</c> as "inherit the job's <c>runner { require … }</c>",
    /// so <c>"require": []</c> is only a second wire spelling of a message that
    /// already has one. And <c>"timeout": ""</c> is not a parseable duration:
    /// honouring it as an explicit override would hand the runner a broken
    /// value where omitting it inherits the job's own timeout.
    /// </remarks>
    internal static TriggerRequest Normalized(
        string jobKey,
        IReadOnlyDictionary<string, object?>? metadata,
        IReadOnlyList<string>? require,
        IReadOnlyList<string>? prefer,
        string? timeout,
        string? idempotencyKey) =>
        new(
            jobKey,
            metadata is { Count: > 0 } ? metadata : null,
            require is { Count: > 0 } ? require : null,
            prefer is { Count: > 0 } ? prefer : null,
            BlankToNull(timeout),
            BlankToNull(idempotencyKey));

    private static string? BlankToNull(string? value) =>
        string.IsNullOrWhiteSpace(value) ? null : value.Trim();
}
