using YamlDotNet.Serialization;
using YamlDotNet.Serialization.NamingConventions;

namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// Loads a <see cref="CaseSpec"/> from a YAML file and normalises body
/// payloads to plain <see cref="Dictionary{TKey, TValue}"/> shapes so they
/// can be serialised back to JSON without YamlDotNet's
/// <c>Dictionary&lt;object, object&gt;</c> noise.
/// </summary>
internal static class CaseLoader
{
    private static readonly IDeserializer _yaml = new DeserializerBuilder()
        .WithNamingConvention(UnderscoredNamingConvention.Instance)
        .IgnoreUnmatchedProperties()
        .Build();

    public static CaseSpec Load(string path)
    {
        var text = File.ReadAllText(path);
        var spec = _yaml.Deserialize<CaseSpec>(text)
            ?? throw new InvalidOperationException($"failed to deserialise case '{path}'");

        // Normalise nested body trees (YamlDotNet hands them back as
        // Dictionary<object, object>) into Dictionary<string, object?> so
        // System.Text.Json can round-trip them cleanly downstream.
        foreach (var entry in spec.ServerScript)
        {
            entry.Respond.Body = NormaliseYamlObject(entry.Respond.Body);
        }
        foreach (var ex in spec.Expectations.Http)
        {
            ex.BodyMatch = NormaliseYamlObject(ex.BodyMatch);
        }

        return spec;
    }

    /// <summary>
    /// Recursively converts YamlDotNet's <c>Dictionary&lt;object, object&gt;</c>
    /// and <c>List&lt;object&gt;</c> trees into JSON-friendly
    /// <c>Dictionary&lt;string, object?&gt;</c> / <c>List&lt;object?&gt;</c>
    /// and coerces numeric strings to long/double where YAML would have
    /// untyped them.
    /// </summary>
    public static object? NormaliseYamlObject(object? node) => node switch
    {
        null => null,
        IDictionary<object, object> dict => dict.ToDictionary(
            kv => kv.Key?.ToString() ?? "",
            kv => NormaliseYamlObject(kv.Value),
            StringComparer.Ordinal),
        IDictionary<string, object> sdict => sdict.ToDictionary(
            kv => kv.Key,
            kv => NormaliseYamlObject(kv.Value),
            StringComparer.Ordinal),
        IEnumerable<object> list when node is not string => list.Select(NormaliseYamlObject).ToList(),
        string s => CoerceScalar(s),
        _ => node,
    };

    private static object CoerceScalar(string s)
    {
        if (long.TryParse(s, System.Globalization.NumberStyles.Integer, System.Globalization.CultureInfo.InvariantCulture, out var l))
        {
            return l;
        }
        if (double.TryParse(s, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var d))
        {
            return d;
        }
        return s;
    }
}
