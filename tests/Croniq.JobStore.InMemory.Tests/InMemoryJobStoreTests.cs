using System;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.Abstractions;
using Xunit;

namespace Croniq.JobStore.InMemory.Tests;

public class InMemoryJobStoreTests
{
    private static readonly PartitionScope DefaultScope = new("1", "dev");

    [Fact]
    public async Task Acquire_and_release_reschedules_trigger()
    {
        var now = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);
        var store = CreateStore(now, 45);

        var jobKey = "1:dev:samples:job";
        await store.UpsertJobAsync(new JobDefinition(jobKey, "samples", "job", null, "sample", null), CancellationToken.None);
        await store.UpsertTriggerAsync(new TriggerDefinition(jobKey, jobKey, "0 * * * * ?", DefaultScope), CancellationToken.None);

        var acquire = new TriggerAcquireRequest(DefaultScope, "instance-1", now.AddMinutes(1), 5);
        var leases = await store.AcquireAsync(acquire, CancellationToken.None);
        var lease = Assert.Single(leases);

        Assert.Equal(now.AddMinutes(1), lease.FireAtUtc);
        Assert.Equal(now.AddMinutes(1).AddSeconds(45), lease.LeaseExpiresAtUtc);

        await store.ReleaseAsync(new TriggerReleaseRequest(lease, true, null), CancellationToken.None);

        var reacquire = new TriggerAcquireRequest(DefaultScope, acquire.InstanceId, now.AddMinutes(2), 5);
        var leases2 = await store.AcquireAsync(reacquire, CancellationToken.None);
        var lease2 = Assert.Single(leases2);
        Assert.True(lease2.FireAtUtc > lease.FireAtUtc);
    }

    [Fact]
    public async Task Reschedules_on_failure_when_next_fire_provided()
    {
        var now = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);
        var store = CreateStore(now, 30);

        var jobKey = "1:dev:samples:job2";
        var triggerKey = $"{jobKey}:t";

        await store.UpsertJobAsync(new JobDefinition(jobKey, "samples", "job2", null, "sample", null), CancellationToken.None);
        await store.UpsertTriggerAsync(new TriggerDefinition(triggerKey, jobKey, "0/30 * * * * ?", DefaultScope), CancellationToken.None);

        var lease = (await store.AcquireAsync(new TriggerAcquireRequest(DefaultScope, "instance-1", now.AddSeconds(30), 5), CancellationToken.None)).First();

        var retryAt = lease.FireAtUtc.AddSeconds(15);
        await store.ReleaseAsync(new TriggerReleaseRequest(lease, false, retryAt, "boom"), CancellationToken.None);

        var reacquired = await store.AcquireAsync(new TriggerAcquireRequest(DefaultScope, "instance-1", retryAt.AddSeconds(1), 5), CancellationToken.None);
        var nextLease = Assert.Single(reacquired);
        Assert.Equal(triggerKey, nextLease.TriggerId);
        Assert.Equal(retryAt, nextLease.FireAtUtc);
    }

    [Fact]
    public async Task Honors_scope_and_reacquires_after_lease_expiry()
    {
        var now = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);
        var store = CreateStore(now, 10);

        var jobA = "1:dev:samples:a";
        var jobB = "2:dev:samples:b";

        await store.UpsertJobAsync(new JobDefinition(jobA, "samples", "a", null, null, null), CancellationToken.None);
        await store.UpsertJobAsync(new JobDefinition(jobB, "samples", "b", null, null, null), CancellationToken.None);

        await store.UpsertTriggerAsync(new TriggerDefinition(jobA, jobA, "0 * * * * ?", DefaultScope), CancellationToken.None);
        await store.UpsertTriggerAsync(new TriggerDefinition(jobB, jobB, "0 * * * * ?", new PartitionScope("2", "dev")), CancellationToken.None);

        var leaseA = (await store.AcquireAsync(new TriggerAcquireRequest(DefaultScope, "i1", now.AddMinutes(1), 5), CancellationToken.None)).Single();
        var leaseB = (await store.AcquireAsync(new TriggerAcquireRequest(new PartitionScope("2", "dev"), "i2", now.AddMinutes(1), 5), CancellationToken.None)).Single();

        Assert.Equal(jobA, leaseA.TriggerId);
        Assert.Equal(jobB, leaseB.TriggerId);

        // Wait out the lease; the trigger should be available again even without an explicit release.
        var reacquire = await store.AcquireAsync(new TriggerAcquireRequest(DefaultScope, "i3", leaseA.FireAtUtc.AddSeconds(11), 5), CancellationToken.None);
        var leaseAfterExpiry = Assert.Single(reacquire);
        Assert.Equal(leaseA.TriggerId, leaseAfterExpiry.TriggerId);
    }

    private static InMemoryJobStore CreateStore(DateTimeOffset? now, int leaseDurationSeconds)
    {
        var options = new InMemoryJobStoreOptions
        {
            LeaseDurationSeconds = leaseDurationSeconds,
            UtcNowProvider = now.HasValue ? () => now.Value : null
        };

        return new InMemoryJobStore(Microsoft.Extensions.Options.Options.Create(options));
    }
}
