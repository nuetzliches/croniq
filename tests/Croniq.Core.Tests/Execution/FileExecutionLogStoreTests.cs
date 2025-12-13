using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Microsoft.Extensions.Logging;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class FileExecutionLogStoreTests : IDisposable
{
    private readonly string _tempDir;

    public FileExecutionLogStoreTests()
    {
        _tempDir = Path.Combine(Path.GetTempPath(), "croniq-tests", Guid.NewGuid().ToString("N"));
    }

    [Fact]
    public async Task Writes_start_log_and_entries_and_completion()
    {
        var options = new FileExecutionLogStoreOptions { BasePath = _tempDir };
        var store = new FileExecutionLogStore(options);

        var record = new ExecutionRecord(
            "exec-123",
            ExecutionKind.Job,
            null,
            "t:dev:ns:job",
            "t",
            "dev",
            "tr-1",
            DateTimeOffset.UtcNow,
            DateTimeOffset.UtcNow,
            "instance-1",
            "trace-id",
            "span-id",
            "corr-1");

        await store.OnExecutionStartedAsync(record, CancellationToken.None);

        var entries = new List<ExecutionLogEntry>
        {
            new(
                "exec-123",
                DateTimeOffset.UtcNow,
                LogLevel.Information,
                "Started {Job}",
                "Started job",
                null,
                new Dictionary<string, object?> { { "Job", "demo" } },
                "trace-id",
                "span-id",
                "corr-1",
                1)
        };

        await store.AppendAsync(record.ExecutionId, entries, CancellationToken.None);

        var completion = new ExecutionCompletion(
            record.ExecutionId,
            DateTimeOffset.UtcNow,
            ExecutionStatus.Succeeded,
            12.3,
            null,
            null);

        await store.OnExecutionCompletedAsync(completion, CancellationToken.None);

        var files = Directory.GetFiles(_tempDir, "*.ndjson", SearchOption.AllDirectories);
        files.ShouldHaveSingleItem();

        var content = await File.ReadAllLinesAsync(files.Single());
        content.Length.ShouldBe(3);
        content[0].ShouldContain("\"type\":\"start\"");
        content[1].ShouldContain("\"type\":\"log\"");
        content[1].ShouldContain("\"messageTemplate\":\"Started {Job}\"");
        content[2].ShouldContain("\"type\":\"completion\"");
    }

    public void Dispose()
    {
        try
        {
            if (Directory.Exists(_tempDir))
            {
                Directory.Delete(_tempDir, recursive: true);
            }
        }
        catch
        {
            // ignore cleanup errors in tests
        }
    }
}
