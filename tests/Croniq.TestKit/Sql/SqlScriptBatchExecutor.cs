using System.Text.RegularExpressions;
using Microsoft.Data.SqlClient;

namespace Croniq.TestKit.Sql;

internal static partial class SqlScriptBatchExecutor
{
    public static async Task ExecuteAsync(SqlConnection connection, string scriptPath, CancellationToken cancellationToken)
    {
        if (!File.Exists(scriptPath))
        {
            throw new FileNotFoundException($"SQL script not found at '{scriptPath}'.", scriptPath);
        }

        var text = await File.ReadAllTextAsync(scriptPath, cancellationToken).ConfigureAwait(false);
        foreach (var batch in SplitBatches(text))
        {
            if (string.IsNullOrWhiteSpace(batch))
            {
                continue;
            }

            await using var command = connection.CreateCommand();
            command.CommandText = batch;
            command.CommandTimeout = 90;
            await command.ExecuteNonQueryAsync(cancellationToken).ConfigureAwait(false);
        }
    }

    private static IEnumerable<string> SplitBatches(string script)
    {
        if (string.IsNullOrWhiteSpace(script))
        {
            yield break;
        }

        var batches = BatchSeparator().Split(script);
        foreach (var batch in batches)
        {
            if (!string.IsNullOrWhiteSpace(batch))
            {
                yield return batch;
            }
        }
    }

    [GeneratedRegex(@"^\s*GO\b.*$", RegexOptions.Multiline | RegexOptions.IgnoreCase)]
    private static partial Regex BatchSeparator();
}
