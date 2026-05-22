using System.Text.Json;

using Croniq.Runner.Sdk.Protocol;

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Per-execution helper that auto-injects <c>job_key</c>, <c>runner_id</c>,
/// and (when set) <c>runner_tags</c> into every <see cref="WorkEvent"/>'s
/// fields map. Explicit caller values take precedence. Port of
/// <c>enrichment.rs</c> from the Rust SDK.
/// </summary>
internal sealed class LogEnrichment
{
    private readonly string _jobKey;
    private readonly string _runnerId;
    private readonly string? _serializedTags;

    public LogEnrichment(string jobKey, string runnerId, IReadOnlyList<string> runnerTags)
    {
        _jobKey = jobKey;
        _runnerId = runnerId;
        _serializedTags = SerializeTags(runnerTags);
    }

    public WorkEvent Enrich(WorkEvent source)
    {
        var fields = new Dictionary<string, string>(source.Fields?.Count + 3 ?? 3, StringComparer.Ordinal);
        if (source.Fields is not null)
        {
            foreach (var kvp in source.Fields)
            {
                fields[kvp.Key] = kvp.Value;
            }
        }

        fields.TryAdd("job_key", _jobKey);
        fields.TryAdd("runner_id", _runnerId);
        if (_serializedTags is not null)
        {
            fields.TryAdd("runner_tags", _serializedTags);
        }

        return source with { Fields = fields };
    }

    private static string? SerializeTags(IReadOnlyList<string> tags)
    {
        if (tags.Count == 0)
        {
            return null;
        }

        // Match the Rust SDK: tags are serialized as a JSON array string.
        // Cheap & predictable — pre-allocate buffer.
        var buffer = new System.IO.MemoryStream(capacity: 32 + (tags.Count * 16));
        using (var writer = new Utf8JsonWriter(buffer))
        {
            writer.WriteStartArray();
            foreach (var tag in tags)
            {
                writer.WriteStringValue(tag);
            }
            writer.WriteEndArray();
        }
        return System.Text.Encoding.UTF8.GetString(buffer.ToArray());
    }
}
