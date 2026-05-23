namespace Croniq.Runner.Sdk.Conformance.Tests;

/// <summary>
/// One xUnit theory entry per YAML case. The discovery scans the
/// <c>cases/</c> folder copied to the output directory at build time
/// (see csproj), so adding a new YAML automatically adds a new test.
/// </summary>
public sealed class ConformanceTests
{
    private static readonly string _casesDir = Path.Combine(AppContext.BaseDirectory, "cases");

    public static IEnumerable<object[]> Cases()
    {
        if (!Directory.Exists(_casesDir))
        {
            yield break;
        }
        foreach (var path in Directory.EnumerateFiles(_casesDir, "*.yaml").OrderBy(p => p))
        {
            yield return new object[] { Path.GetFileName(path) };
        }
    }

    [Theory]
    [MemberData(nameof(Cases))]
    public async Task Conformance(string caseFile)
    {
        var path = Path.Combine(_casesDir, caseFile);
        var spec = CaseLoader.Load(path);
        await ConformanceRunner.RunAsync(spec);
    }
}
