using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.JobStore.InMemory.Tests;

public sealed class InMemoryWorkerStoreTests
{
    [Fact]
    public async Task UpsertHeartbeatAsync_stores_worker_and_lists_online_status()
    {
        var store = new InMemoryWorkerStore(Options.Create(new WorkerStoreOptions
        {
            OnlineTtl = TimeSpan.FromMinutes(5)
        }));

        var scope = new PartitionScope("tenant-1", "dev");
        var seenAt = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);

        await store.UpsertHeartbeatAsync(
            new WorkerHeartbeat(scope, "worker-1", seenAt, "{\"kind\":\"worker\"}"),
            CancellationToken.None);

        var results = await store.ListAsync(new WorkerQuery(scope, seenAt.AddMinutes(1)), CancellationToken.None);

        var worker = results.ShouldHaveSingleItem();
        worker.InstanceId.ShouldBe("worker-1");
        worker.IsOnline.ShouldBeTrue();
        worker.MetadataJson.ShouldBe("{\"kind\":\"worker\"}");
    }

    [Fact]
    public async Task ListAsync_prunes_expired_workers()
    {
        var store = new InMemoryWorkerStore(Options.Create(new WorkerStoreOptions
        {
            OnlineTtl = TimeSpan.FromMinutes(1)
        }));

        var scope = new PartitionScope("tenant-1", "dev");
        var seenAt = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);

        await store.UpsertHeartbeatAsync(
            new WorkerHeartbeat(scope, "worker-1", seenAt, null),
            CancellationToken.None);

        var results = await store.ListAsync(new WorkerQuery(scope, seenAt.AddMinutes(2)), CancellationToken.None);

        results.ShouldBeEmpty();
    }
}
