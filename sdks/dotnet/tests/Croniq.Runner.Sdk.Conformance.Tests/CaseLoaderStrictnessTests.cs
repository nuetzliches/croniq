using YamlDotNet.Core;

namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// Guards the loader's own failure mode. A loader that silently drops
/// unrecognised keys goes green exactly when the contract stops being
/// enforced (#460), so the strictness needs a test that provokes the silence
/// and asserts it is now noisy — otherwise re-adding
/// <c>IgnoreUnmatchedProperties()</c> would be an invisible regression.
/// </summary>
public sealed class CaseLoaderStrictnessTests
{
    private const string MinimalCase = """
        name: strictness probe
        runner_config:
          capabilities: ["work"]
        handlers:
          - job_key: "work:probe"
            behavior: noop
        server_script:
          - on: "POST /v1/work/poll"
            respond:
              status: 200
              body: { work: [], cancel: [] }
        expectations:
          duration_max_ms: 500
          http:
            - method: POST
              path: /v1/work/poll
              min_count: 1
        """;

    [Theory]
    // A key the schema could grow next, at each level a case nests.
    [InlineData("name: strictness probe", "name: strictness probe\nnot_a_real_key: 1")]
    [InlineData("  capabilities: [\"work\"]", "  capabilities: [\"work\"]\n  not_a_real_key: 1")]
    [InlineData("    behavior: noop", "    behavior: noop\n    not_a_real_key: 1")]
    [InlineData("      status: 200", "      status: 200\n      not_a_real_key: 1")]
    [InlineData("      min_count: 1", "      min_count: 1\n      not_a_real_key: 1")]
    public void Load_rejects_a_key_the_binding_does_not_model(string anchor, string replacement)
    {
        var yaml = ReplaceOnce(MinimalCase, anchor, replacement);
        var path = WriteTemp(yaml);

        var ex = Should.Throw<YamlException>(() => CaseLoader.Load(path));
        ex.Message.ShouldContain("not_a_real_key");
    }

    /// <summary>
    /// Counterweight: strictness must reject the unknown without rejecting
    /// what the corpus legitimately uses. Keeps the fixture above honest — a
    /// fixture that failed to load on its own would make the negative cases
    /// pass for the wrong reason.
    /// </summary>
    [Fact]
    public void Load_accepts_the_known_vocabulary()
    {
        var spec = CaseLoader.Load(WriteTemp(MinimalCase));

        spec.Name.ShouldBe("strictness probe");
        spec.Handlers.Count.ShouldBe(1);
        spec.Expectations.Http.Count.ShouldBe(1);
    }

    private static string ReplaceOnce(string text, string anchor, string replacement)
    {
        var idx = text.IndexOf(anchor, StringComparison.Ordinal);
        idx.ShouldBeGreaterThanOrEqualTo(0, $"fixture must contain the anchor '{anchor}'");
        return string.Concat(text.AsSpan(0, idx), replacement, text.AsSpan(idx + anchor.Length));
    }

    private static string WriteTemp(string yaml)
    {
        var path = Path.Combine(Path.GetTempPath(), $"croniq-case-{Guid.NewGuid():N}.yaml");
        File.WriteAllText(path, yaml);
        return path;
    }
}
