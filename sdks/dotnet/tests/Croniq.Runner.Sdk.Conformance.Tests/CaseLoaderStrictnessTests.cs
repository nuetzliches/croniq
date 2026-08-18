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

    /// <summary>
    /// The loader must tolerate concurrent callers.
    /// </summary>
    /// <remarks>
    /// xUnit parallelises across test collections, and a class is a collection:
    /// adding this test class alongside <see cref="ConformanceTests"/> meant two
    /// collections calling <c>CaseLoader.Load</c> at once for the first time. A
    /// single shared <c>IDeserializer</c> did not survive that — YamlDotNet threw
    /// <c>Exception during deserialization</c> wrapping *"Operations that change
    /// non-concurrent collections must have exclusive access"* from its internal
    /// type-descriptor cache. It reproduced on CI, not on an 8-core dev box, so
    /// this test provokes the race directly rather than relying on scheduling
    /// luck. Deleting it would let the loader silently regress to a shared
    /// instance whose failure only appears in CI.
    /// </remarks>
    [Fact]
    public void Load_is_safe_under_concurrent_use()
    {
        var path = WriteTemp(MinimalCase);

        var specs = new CaseSpec[64];
        Parallel.For(0, specs.Length, i => specs[i] = CaseLoader.Load(path));

        specs.ShouldAllBe(spec => spec.Name == "strictness probe");
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
