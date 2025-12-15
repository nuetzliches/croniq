using System;
using System.IO;
using System.Reflection;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public class ExecutionLogRetentionServiceTests
{
    [Fact]
    public void Sweep_returns_when_base_directory_does_not_exist()
    {
        var missingBasePath = Path.Combine(Path.GetTempPath(), "croniq-retention-tests", Guid.NewGuid().ToString("N"));

        var storeOptions = Microsoft.Extensions.Options.Options.Create(new FileExecutionLogStoreOptions { BasePath = missingBasePath });
        var retentionOptions = Microsoft.Extensions.Options.Options.Create(new ExecutionLogRetentionOptions { RetentionDays = 5, SweepInterval = TimeSpan.FromHours(1) });
        var service = new ExecutionLogRetentionService(storeOptions, retentionOptions, NullLogger<ExecutionLogRetentionService>.Instance);

        InvokeSweep(service);

        Directory.Exists(missingBasePath).ShouldBeFalse();
    }

    [Fact]
    public void Sweep_deletes_old_files_and_cleans_empty_directories()
    {
        var tempDir = Path.Combine(Path.GetTempPath(), "croniq-retention-tests", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(tempDir);

        try
        {
            var deleteDir = Path.Combine(tempDir, "tenant-a", "prod");
            Directory.CreateDirectory(deleteDir);
            var keepDir = Path.Combine(tempDir, "tenant-b", "prod");
            Directory.CreateDirectory(keepDir);

            var old = Path.Combine(deleteDir, "old.ndjson");
            File.WriteAllText(old, "old");
            File.SetLastWriteTimeUtc(old, DateTime.UtcNow.AddDays(-10));

            var recent = Path.Combine(keepDir, "recent.ndjson");
            File.WriteAllText(recent, "recent");
            File.SetLastWriteTimeUtc(recent, DateTime.UtcNow.AddDays(-1));

            var storeOptions = Microsoft.Extensions.Options.Options.Create(new FileExecutionLogStoreOptions { BasePath = tempDir });
            var retentionOptions = Microsoft.Extensions.Options.Options.Create(new ExecutionLogRetentionOptions { RetentionDays = 5, SweepInterval = TimeSpan.FromHours(1) });
            var service = new ExecutionLogRetentionService(storeOptions, retentionOptions, NullLogger<ExecutionLogRetentionService>.Instance);

            InvokeSweep(service);

            File.Exists(old).ShouldBeFalse();
            File.Exists(recent).ShouldBeTrue();

            Directory.Exists(deleteDir).ShouldBeFalse();
            Directory.Exists(Path.Combine(tempDir, "tenant-a")).ShouldBeFalse();
            Directory.Exists(keepDir).ShouldBeTrue();
        }
        finally
        {
            if (Directory.Exists(tempDir))
            {
                Directory.Delete(tempDir, true);
            }
        }
    }

    private static void InvokeSweep(ExecutionLogRetentionService service)
    {
        var method = typeof(ExecutionLogRetentionService).GetMethod("Sweep", BindingFlags.Instance | BindingFlags.NonPublic);
        method.ShouldNotBeNull();
        method!.Invoke(service, null);
    }
}
