namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// One xUnit theory entry per trigger (producer) YAML case. Discovery scans
/// the <c>cases-trigger/</c> folder copied to the output directory at build
/// time (see csproj), so adding a new YAML automatically adds a new test.
/// </summary>
/// <remarks>
/// Added in issue #554: .NET was the only SDK that ran no trigger cases at
/// all, so the <c>body_absent</c> contract — "a producer must not fabricate
/// defaults on the wire", which #551 depended on and #553 extended to
/// explicitly empty values — rested here on unit tests alone rather than the
/// shared corpus.
/// </remarks>
public sealed class TriggerConformanceTests
{
    private static readonly string _casesDir = Path.Combine(AppContext.BaseDirectory, "cases-trigger");

    public static IEnumerable<object[]> Cases()
    {
        if (!Directory.Exists(_casesDir))
        {
            yield break;
        }
        foreach (var path in Directory.EnumerateFiles(_casesDir, "*.yaml").OrderBy(p => p, StringComparer.Ordinal))
        {
            yield return new object[] { Path.GetFileName(path) };
        }
    }

    [Theory]
    [MemberData(nameof(Cases))]
    public async Task TriggerConformance(string caseFile)
    {
        var path = Path.Combine(_casesDir, caseFile);
        var spec = TriggerCaseLoader.Load(path);
        await TriggerConformanceRunner.RunAsync(spec);
    }

    /// <summary>
    /// Guards the discovery itself: an empty <see cref="Cases"/> would make the
    /// theory above vacuously green, which is the exact shape of the gap #554
    /// closed. Pinned to the corpus size so a case that stops being copied to
    /// the output — or a csproj glob that silently stops matching — fails
    /// loudly instead of quietly reducing coverage to nothing.
    /// </summary>
    [Fact]
    public void EveryTriggerCaseInTheCorpusIsDiscovered()
    {
        var discovered = Cases().Select(c => (string)c[0]).ToList();

        Assert.True(
            Directory.Exists(_casesDir),
            $"cases-trigger/ must be copied to the output directory; looked in {_casesDir}");
        Assert.True(
            discovered.Count >= 12,
            $"expected the full trigger corpus (>= 12 cases), found {discovered.Count}: {string.Join(", ", discovered)}");
        Assert.Contains("01-trigger-minimal.yaml", discovered);
        Assert.Contains("12-trigger-empty-optionals.yaml", discovered);
    }
}
