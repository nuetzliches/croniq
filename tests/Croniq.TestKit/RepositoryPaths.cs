using System.IO;

namespace Croniq.TestKit;

internal static class RepositoryPaths
{
    public static string GetRoot()
    {
        var current = new DirectoryInfo(AppContext.BaseDirectory);
        while (current is not null)
        {
            var solutionFile = Path.Combine(current.FullName, "croniq.sln");
            if (File.Exists(solutionFile))
            {
                return current.FullName;
            }

            current = current.Parent;
        }

        throw new InvalidOperationException("Could not locate repository root (croniq.sln not found).");
    }

    public static string GetXtraqSqlRoot()
    {
        return Path.Combine(GetRoot(), "infra", "sql", "xtraq");
    }
}
