using System;
using System.IO;
using System.Linq;
using System.Reflection;

namespace Croniq.TestKit.Infrastructure;

/// <summary>
/// Resolves paths relative to the Croniq repository root so tests can place artifacts deterministically.
/// </summary>
public static class RepositoryLocator
{
    private const string SolutionFileName = "croniq.slnx";
    private static readonly Lazy<string> RootResolver = new(ResolveRoot);

    public static string Root => RootResolver.Value;

    public static string GetArtifactsDirectory(string? relativeSegment = null)
    {
        var segment = string.IsNullOrWhiteSpace(relativeSegment)
            ? Array.Empty<string>()
            : relativeSegment!
                .Split(new[] { "/", "\\" }, StringSplitOptions.RemoveEmptyEntries)
                .ToArray();

        var path = Path.Combine(new[] { Root, "artifacts" }.Concat(segment).ToArray());
        Directory.CreateDirectory(path);
        return path;
    }

    private static string ResolveRoot()
    {
        var directory = new DirectoryInfo(AppContext.BaseDirectory);
        while (directory is not null)
        {
            var solution = Path.Combine(directory.FullName, SolutionFileName);
            if (File.Exists(solution))
            {
                return directory.FullName;
            }

            directory = directory.Parent;
        }

        throw new InvalidOperationException($"Unable to locate solution file '{SolutionFileName}' starting from '{AppContext.BaseDirectory}'.");
    }
}
