using YamlDotNet.Core;

namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// The <see cref="TriggerCaseLoader"/> counterpart to
/// <see cref="CaseLoaderStrictnessTests"/>. A loader that silently drops
/// unrecognised keys goes green exactly when the contract stops being enforced
/// (#460), so the strictness needs a test that provokes the silence and asserts
/// it is noisy — otherwise adding <c>IgnoreUnmatchedProperties()</c> would be an
/// invisible regression.
/// </summary>
/// <remarks>
/// This matters more for the trigger corpus than the runner one: #554 existed
/// because this binding ran no trigger cases at all, and the next-worst version
/// of that gap is a binding that loads them but asserts less than they declare.
///
/// Anchor indentation below is the <em>post-strip</em> depth: a C# raw string
/// literal removes the common indentation shared with its closing delimiter, so
/// the YAML the loader sees is dedented by 8 relative to this file.
/// </remarks>
public sealed class TriggerCaseLoaderStrictnessTests
{
    private const string MinimalCase = """
        name: trigger strictness probe
        trigger_config:
          api_key: "croniq_trigger_key"
        trigger_calls:
          - request:
              job_key: "billing:invoice"
            expect:
              response:
                execution_id: "exec-1"
                queued: 1
        server_script:
          - on: "POST /v1/trigger"
            respond:
              status: 200
              body: { execution_id: "exec-1", queued: 1 }
        expectations:
          duration_max_ms: 2000
          http:
            - method: POST
              path: /v1/trigger
              exact_count: 1
              body_absent:
                - timeout
        """;

    [Theory]
    // A key the schema could grow next, at each level a trigger case nests.
    [InlineData("name: trigger strictness probe")]
    [InlineData("  api_key: \"croniq_trigger_key\"")]
    [InlineData("      job_key: \"billing:invoice\"")]
    [InlineData("        queued: 1")]
    [InlineData("      status: 200")]
    [InlineData("      exact_count: 1")]
    public void Load_rejects_a_key_the_binding_does_not_model(string anchor)
    {
        // Inject an unmodelled sibling at the anchor's own indentation, so the
        // stray key lands in the same mapping the anchor belongs to.
        var indent = new string(' ', anchor.Length - anchor.TrimStart().Length);
        var yaml = ReplaceOnce(MinimalCase, anchor, $"{anchor}\n{indent}not_a_real_key: 1");
        var path = WriteTemp(yaml);

        var ex = Should.Throw<YamlException>(() => TriggerCaseLoader.Load(path));
        ex.Message.ShouldContain("not_a_real_key");
    }

    /// <summary>
    /// Counterweight: strictness must reject the unknown without rejecting what
    /// the corpus legitimately uses. Also pins that <c>body_absent</c> — the
    /// assertion this binding was missing entirely — actually reaches the spec
    /// rather than being tolerated and dropped.
    /// </summary>
    [Fact]
    public void Load_accepts_the_known_vocabulary()
    {
        var spec = TriggerCaseLoader.Load(WriteTemp(MinimalCase));

        spec.Name.ShouldBe("trigger strictness probe");
        spec.TriggerConfig.ApiKey.ShouldBe("croniq_trigger_key");
        spec.TriggerCalls.Count.ShouldBe(1);
        spec.TriggerCalls[0].Request.JobKey.ShouldBe("billing:invoice");
        spec.TriggerCalls[0].Expect.Response!.Queued.ShouldBe(1);
        spec.Expectations.Http.Count.ShouldBe(1);
        spec.Expectations.Http[0].BodyAbsent.ShouldBe(["timeout"]);
    }

    /// <summary>
    /// An omitted optional must stay distinguishable from one supplied empty:
    /// case <c>12-trigger-empty-optionals</c> passes empty values deliberately
    /// (#553), so binding them to non-null defaults would make that case
    /// vacuous — it would assert omission of something the binding never sent.
    /// </summary>
    [Fact]
    public void Load_keeps_unset_request_optionals_null()
    {
        var spec = TriggerCaseLoader.Load(WriteTemp(MinimalCase));
        var request = spec.TriggerCalls[0].Request;

        request.Require.ShouldBeNull();
        request.Prefer.ShouldBeNull();
        request.Metadata.ShouldBeNull();
        request.Timeout.ShouldBeNull();
        request.IdempotencyKey.ShouldBeNull();
    }

    /// <summary>
    /// Explicitly empty values survive the load as empty rather than being
    /// coerced to null — the other half of the distinction above, and what makes
    /// the #553 case actually exercise the SDK's normalization instead of
    /// asserting omission of something never supplied.
    /// </summary>
    [Fact]
    public void Load_keeps_explicitly_empty_request_optionals_empty()
    {
        var anchor = "      job_key: \"billing:invoice\"";
        var yaml = ReplaceOnce(
            MinimalCase,
            anchor,
            anchor
                + "\n      require: []"
                + "\n      prefer: []"
                + "\n      metadata: {}"
                + "\n      timeout: \"\"");
        var spec = TriggerCaseLoader.Load(WriteTemp(yaml));
        var request = spec.TriggerCalls[0].Request;

        request.Require.ShouldNotBeNull().ShouldBeEmpty();
        request.Prefer.ShouldNotBeNull().ShouldBeEmpty();
        request.Metadata.ShouldNotBeNull().ShouldBeEmpty();
        request.Timeout.ShouldBe("");
    }

    /// <summary>
    /// The loader must tolerate concurrent callers — see the note on
    /// <see cref="CaseLoaderStrictnessTests.Load_is_safe_under_concurrent_use"/>
    /// for the YamlDotNet type-descriptor race this provokes. Adding a second
    /// loader made that reachable again from a fresh code path.
    /// </summary>
    [Fact]
    public void Load_is_safe_under_concurrent_use()
    {
        var path = WriteTemp(MinimalCase);

        var specs = new TriggerCaseSpec[64];
        Parallel.For(0, specs.Length, i => specs[i] = TriggerCaseLoader.Load(path));

        specs.ShouldAllBe(spec => spec.Name == "trigger strictness probe");
    }

    private static string ReplaceOnce(string text, string anchor, string replacement)
    {
        var idx = text.IndexOf(anchor, StringComparison.Ordinal);
        idx.ShouldBeGreaterThanOrEqualTo(0, $"fixture must contain the anchor '{anchor}'");
        return string.Concat(text.AsSpan(0, idx), replacement, text.AsSpan(idx + anchor.Length));
    }

    private static string WriteTemp(string yaml)
    {
        var path = Path.Combine(Path.GetTempPath(), $"croniq-trigger-case-{Guid.NewGuid():N}.yaml");
        File.WriteAllText(path, yaml);
        return path;
    }
}
