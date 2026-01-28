using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.JobStore.InMemory.Tests;

public sealed class InMemoryRunnerStoreTests
{
    [Fact]
    public async Task UpsertHeartbeatAsync_stores_runner_and_lists_online_status()
    {
        var store = new InMemoryRunnerStore(Microsoft.Extensions.Options.Options.Create(new RunnerStoreOptions
        {
            OnlineTtl = TimeSpan.FromMinutes(5),
            OfflineRetentionTtl = TimeSpan.FromMinutes(15)
        }));

        var scope = new PartitionScope("tenant-1", "dev");
        var seenAt = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);

        await store.UpsertHeartbeatAsync(
            new RunnerHeartbeat(scope, "runner-1", seenAt, "{\"kind\":\"http\"}"),
            CancellationToken.None);

        var results = await store.ListAsync(new RunnerQuery(scope, seenAt.AddMinutes(1)), CancellationToken.None);

        var runner = results.ShouldHaveSingleItem();
        runner.RunnerId.ShouldBe("runner-1");
        runner.IsOnline.ShouldBeTrue();
        runner.MetadataJson.ShouldBe("{\"kind\":\"http\"}");
    }

    [Fact]
    public async Task ListAsync_prunes_expired_runners()
    {
        var store = new InMemoryRunnerStore(Microsoft.Extensions.Options.Options.Create(new RunnerStoreOptions
        {
            OnlineTtl = TimeSpan.FromMinutes(1),
            OfflineRetentionTtl = TimeSpan.FromMinutes(1)
        }));

        var scope = new PartitionScope("tenant-1", "dev");
        var seenAt = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);

        await store.UpsertHeartbeatAsync(
            new RunnerHeartbeat(scope, "runner-1", seenAt, null),
            CancellationToken.None);

        var results = await store.ListAsync(new RunnerQuery(scope, seenAt.AddMinutes(2)), CancellationToken.None);

        results.ShouldBeEmpty();
    }

    [Fact]
    public async Task ListAsync_includeOffline_returns_offline_runner()
    {
        var store = new InMemoryRunnerStore(Microsoft.Extensions.Options.Options.Create(new RunnerStoreOptions
        {
            OnlineTtl = TimeSpan.FromMinutes(1),
            OfflineRetentionTtl = TimeSpan.FromMinutes(10)
        }));

        var scope = new PartitionScope("tenant-1", "dev");
        var seenAt = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);

        await store.UpsertHeartbeatAsync(
            new RunnerHeartbeat(scope, "runner-1", seenAt, null),
            CancellationToken.None);

        var results = await store.ListAsync(
            new RunnerQuery(scope, seenAt.AddMinutes(2), IncludeOffline: true),
            CancellationToken.None);

        var runner = results.ShouldHaveSingleItem();
        runner.RunnerId.ShouldBe("runner-1");
        runner.IsOnline.ShouldBeFalse();
    }
}
