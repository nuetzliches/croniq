using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Core.Options;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class ExecutionLogRetentionServiceTests
{
    [Fact]
    public async Task Deletes_files_older_than_retention_and_keeps_recent()
    {
        var tempDir = Path.Combine(Path.GetTempPath(), "croniq-retention-tests", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(tempDir);
        try
        {
            var recent = Path.Combine(tempDir, "recent.ndjson");
            var old = Path.Combine(tempDir, "old.ndjson");
            await File.WriteAllTextAsync(recent, "recent");
            await File.WriteAllTextAsync(old, "old");

            File.SetLastWriteTimeUtc(old, DateTime.UtcNow.AddDays(-10));
            File.SetLastWriteTimeUtc(recent, DateTime.UtcNow.AddDays(-1));

            var storeOptions = Microsoft.Extensions.Options.Options.Create(new FileExecutionLogStoreOptions { BasePath = tempDir });
            var retentionOptions = Microsoft.Extensions.Options.Options.Create(new ExecutionLogRetentionOptions { RetentionDays = 5, SweepInterval = TimeSpan.FromHours(1) });
            var service = new ExecutionLogRetentionService(storeOptions, retentionOptions, NullLogger<ExecutionLogRetentionService>.Instance);

            // invoke Sweep indirectly via ExecuteAsync iteration
            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(1));
            await service.StartAsync(cts.Token);
            await Task.Delay(TimeSpan.FromMilliseconds(200), cts.Token);
            await service.StopAsync(cts.Token);

            File.Exists(old).ShouldBeFalse();
            File.Exists(recent).ShouldBeTrue();
        }
        finally
        {
            if (Directory.Exists(tempDir))
            {
                Directory.Delete(tempDir, true);
            }
        }
    }
}
