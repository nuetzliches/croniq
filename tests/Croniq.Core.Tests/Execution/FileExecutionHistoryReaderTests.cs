using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Execution;
using Croniq.Persistence.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Execution;

public sealed class FileExecutionHistoryReaderTests : IAsyncLifetime
{
    private readonly string _basePath = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N"));
    private readonly CancellationToken _cancellationToken = CancellationToken.None;
    private FileExecutionLogStoreOptions _options = default!;
    private FileExecutionLogStore _store = default!;
    private FileExecutionHistoryReader _reader = default!;

    public Task InitializeAsync()
    {
        Directory.CreateDirectory(_basePath);
        _options = new FileExecutionLogStoreOptions { BasePath = _basePath };
        _store = new FileExecutionLogStore(_options);
        _reader = new FileExecutionHistoryReader(_options);
        return Task.CompletedTask;
    }

    public Task DisposeAsync()
    {
        try
        {
            if (Directory.Exists(_basePath))
            {
                Directory.Delete(_basePath, recursive: true);
            }
        }
        catch
        {
            // ignored
        }

        return Task.CompletedTask;
    }

    [Fact]
    public async Task ListExecutionsReturnsLatestForScope()
    {
        var scope = new PartitionScope("tenant-a", "dev");
        await WriteExecutionAsync(scope, "tenant-a:dev:ops:first", ExecutionStatus.Succeeded, startedOffsetSeconds: -90, durationMs: 1200);
        var recent = await WriteExecutionAsync(scope, "tenant-a:dev:ops:second", ExecutionStatus.Failed, startedOffsetSeconds: -30, durationMs: 3000);

        var summaries = await _reader.ListExecutionsAsync(scope, new ExecutionHistoryQuery { Limit = 10 }, _cancellationToken);

        summaries.Count.ShouldBe(2);
        summaries[0].ExecutionId.ShouldBe(recent.ExecutionId);
        summaries[0].Status.ShouldBe(ExecutionStatus.Failed);
        summaries[0].DurationMs.ShouldBe(3000);
    }

    [Fact]
    public async Task ListExecutionsHonorsFilters()
    {
        var scope = new PartitionScope("tenant-b", "prod");
        await WriteExecutionAsync(scope, "tenant-b:prod:ops:alpha", ExecutionStatus.Succeeded, startedOffsetSeconds: -300);
        var match = await WriteExecutionAsync(scope, "tenant-b:prod:ops:beta", ExecutionStatus.Failed, startedOffsetSeconds: -120);

        var query = new ExecutionHistoryQuery
        {
            JobKey = match.JobKey,
            Status = ExecutionStatus.Failed,
            StartedAfterUtc = match.StartedAtUtc.AddMinutes(-5),
            StartedBeforeUtc = match.StartedAtUtc.AddMinutes(5),
            Limit = 5
        };

        var summaries = await _reader.ListExecutionsAsync(scope, query, _cancellationToken);
        summaries.Count.ShouldBe(1);
        summaries[0].ExecutionId.ShouldBe(match.ExecutionId);
    }

    [Fact]
    public async Task GetExecutionReturnsSummary()
    {
        var scope = new PartitionScope("tenant-c", "test");
        var record = await WriteExecutionAsync(scope, "tenant-c:test:ops:single", ExecutionStatus.Succeeded, startedOffsetSeconds: -10, durationMs: 800);

        var summary = await _reader.GetExecutionAsync(record.ExecutionId, _cancellationToken);
        summary.ShouldNotBeNull();
        summary!.TenantId.ShouldBe(scope.TenantId);
        summary.EnvironmentTag.ShouldBe(scope.EnvironmentTag);
        summary.JobKey.ShouldBe(record.JobKey);
        summary.Status.ShouldBe(ExecutionStatus.Succeeded);
        summary.DurationMs.ShouldBe(800);
    }

    private async Task<ExecutionRecord> WriteExecutionAsync(PartitionScope scope, string jobKey, ExecutionStatus status, int startedOffsetSeconds, double? durationMs = null)
    {
        var executionId = Guid.NewGuid().ToString("N");
        var startedAt = DateTimeOffset.UtcNow.AddSeconds(startedOffsetSeconds);
        var record = new ExecutionRecord(
            executionId,
            ExecutionKind.Job,
            WorkflowId: null,
            jobKey,
            scope.TenantId,
            scope.EnvironmentTag,
            TriggerId: Guid.NewGuid().ToString("N"),
            FireAtUtc: startedAt,
            StartedAtUtc: startedAt,
            InstanceId: "node-1",
            TraceId: Guid.NewGuid().ToString("N"),
            SpanId: Guid.NewGuid().ToString("N"),
            CorrelationId: Guid.NewGuid().ToString("N"));

        await _store.OnExecutionStartedAsync(record, _cancellationToken);
        var completion = new ExecutionCompletion(
            executionId,
            startedAt.AddMilliseconds(durationMs ?? 0),
            status,
            durationMs,
            status == ExecutionStatus.Failed ? "System.Exception" : null,
            status == ExecutionStatus.Failed ? "boom" : null);
        await _store.OnExecutionCompletedAsync(completion, _cancellationToken);
        return record;
    }
}
