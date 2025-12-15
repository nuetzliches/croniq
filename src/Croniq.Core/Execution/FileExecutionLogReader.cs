using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;

namespace Croniq.Core.Execution;

/// <summary>
/// Reads NDJSON execution logs from the filesystem by scanning for the executionId file.
/// </summary>
public sealed class FileExecutionLogReader : IExecutionLogReader
{
    private readonly FileExecutionLogStoreOptions _options;

    public FileExecutionLogReader(FileExecutionLogStoreOptions options)
    {
        _options = options ?? throw new ArgumentNullException(nameof(options));
    }

    public async IAsyncEnumerable<string> ReadLinesAsync(string executionId, [System.Runtime.CompilerServices.EnumeratorCancellation] CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(executionId))
        {
            yield break;
        }

        var basePath = FileExecutionLogPathHelper.ResolveBasePath(_options);
        if (!Directory.Exists(basePath))
        {
            yield break;
        }

        string? filePath = null;
        foreach (var file in Directory.EnumerateFiles(basePath, $"{executionId}.ndjson", SearchOption.AllDirectories))
        {
            filePath = file;
            break;
        }

        if (filePath is null || !File.Exists(filePath))
        {
            yield break;
        }

        using var reader = new StreamReader(File.OpenRead(filePath));
        while (!reader.EndOfStream && !cancellationToken.IsCancellationRequested)
        {
            var line = await reader.ReadLineAsync().ConfigureAwait(false);
            if (line is not null)
            {
                yield return line;
            }
        }
    }
}
