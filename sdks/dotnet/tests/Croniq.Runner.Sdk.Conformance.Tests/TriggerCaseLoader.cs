using YamlDotNet.Serialization;
using YamlDotNet.Serialization.NamingConventions;

namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// Loads a <see cref="TriggerCaseSpec"/> from a <c>cases-trigger/</c> YAML file,
/// normalising nested payload trees the way <see cref="CaseLoader"/> does for
/// runner cases.
/// </summary>
internal static class TriggerCaseLoader
{
    /// <summary>
    /// Builds the deserializer for one <see cref="Load"/> call.
    /// </summary>
    /// <remarks>
    /// Built <b>without</b> <c>IgnoreUnmatchedProperties()</c>, for the same
    /// reason <see cref="CaseLoader"/> is: a key this binding does not model
    /// must be a load-time error rather than a silent drop (#460). A case using
    /// an assertion key the binding never implemented would otherwise load
    /// cleanly and simply not be asserted — a green suite for an unenforced
    /// contract, which is precisely the hole #554 was filed for.
    ///
    /// Also built per call rather than cached: YamlDotNet memoises type
    /// descriptors in a non-concurrent dictionary, so a shared instance
    /// corrupts its own cache when two test classes deserialise at once. See
    /// the note on <see cref="CaseLoader"/>.
    /// </remarks>
    private static IDeserializer BuildDeserializer() => new DeserializerBuilder()
        .WithNamingConvention(UnderscoredNamingConvention.Instance)
        .Build();

    public static TriggerCaseSpec Load(string path)
    {
        var text = File.ReadAllText(path);
        var spec = BuildDeserializer().Deserialize<TriggerCaseSpec>(text)
            ?? throw new InvalidOperationException($"failed to deserialise trigger case '{path}'");

        foreach (var entry in spec.ServerScript)
        {
            entry.Respond.Body = CaseLoader.NormaliseYamlObject(entry.Respond.Body);
        }
        foreach (var ex in spec.Expectations.Http)
        {
            ex.BodyMatch = CaseLoader.NormaliseYamlObject(ex.BodyMatch);
        }

        // Request metadata is handed to the SDK, not matched against, so it
        // needs the same Dictionary<object, object> → Dictionary<string, object?>
        // normalisation before System.Text.Json sees it.
        foreach (var call in spec.TriggerCalls)
        {
            if (call.Request.Metadata is null)
            {
                continue;
            }
            call.Request.Metadata = call.Request.Metadata.ToDictionary(
                kv => kv.Key,
                kv => CaseLoader.NormaliseYamlObject(kv.Value),
                StringComparer.Ordinal);
        }

        return spec;
    }
}
