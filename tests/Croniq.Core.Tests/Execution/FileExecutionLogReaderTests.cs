using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class FileExecutionLogReaderTests
{
    [Fact]
    public async Task Returns_lines_when_file_exists()
    {
        var basePath = Path.Combine(Path.GetTempPath(), "croniq-reader-tests", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(basePath);
        var file = Path.Combine(basePath, "exec-1.ndjson");
        await File.WriteAllTextAsync(file, "line1\nline2");

        var reader = new FileExecutionLogReader(new FileExecutionLogStoreOptions { BasePath = basePath });
        var lines = reader.ReadLinesAsync("exec-1", CancellationToken.None);
        var collected = new System.Collections.Generic.List<string>();
        await foreach (var line in lines)
        {
            collected.Add(line);
        }

        collected.Count.ShouldBe(2);
        collected[0].ShouldBe("line1");
        collected[1].ShouldBe("line2");

        Directory.Delete(basePath, true);
    }

    [Fact]
    public async Task Returns_empty_when_file_missing()
    {
        var basePath = Path.Combine(Path.GetTempPath(), "croniq-reader-tests", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(basePath);
        var reader = new FileExecutionLogReader(new FileExecutionLogStoreOptions { BasePath = basePath });
        var lines = reader.ReadLinesAsync("missing", CancellationToken.None);
        var enumerated = false;
        await foreach (var _ in lines)
        {
            enumerated = true;
        }

        enumerated.ShouldBeFalse();
        Directory.Delete(basePath, true);
    }
}
