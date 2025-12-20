using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Scheduling;
using Croniq.JobStore.InMemory;
using Croniq.Persistence.Abstractions;
using Shouldly;
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
        var lease = leases.ShouldHaveSingleItem();

        lease.FireAtUtc.ShouldBe(now.AddMinutes(1));
        lease.LeaseExpiresAtUtc.ShouldBe(now.AddMinutes(1).AddSeconds(45));

        await store.ReleaseAsync(new TriggerReleaseRequest(lease, true, null), CancellationToken.None);

        var reacquire = new TriggerAcquireRequest(DefaultScope, acquire.InstanceId, now.AddMinutes(2), 5);
        var leases2 = await store.AcquireAsync(reacquire, CancellationToken.None);
        var lease2 = leases2.ShouldHaveSingleItem();
        lease2.FireAtUtc.ShouldBeGreaterThan(lease.FireAtUtc);
    }

    [Fact]
    public async Task Renew_extends_active_lease()
    {
        var now = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);
        var store = CreateStore(now, 30);

        var jobKey = "1:dev:samples:renew";
        var triggerId = $"{jobKey}:t1";

        await store.UpsertJobAsync(new JobDefinition(jobKey, "samples", "renew", null, "sample", null), CancellationToken.None);
        await store.UpsertTriggerAsync(new TriggerDefinition(triggerId, jobKey, "0 * * * * ?", DefaultScope), CancellationToken.None);

        var acquireAt = now.AddMinutes(1);
        var lease = (await store.AcquireAsync(new TriggerAcquireRequest(DefaultScope, "instance-1", acquireAt, 1), CancellationToken.None))
            .ShouldHaveSingleItem();

        var renewAt = acquireAt.AddSeconds(5);
        var renewed = await store.TryRenewLeaseAsync(new TriggerLeaseRenewRequest(lease, "instance-1", renewAt), CancellationToken.None);

        renewed.ShouldNotBeNull();
        renewed!.LeaseExpiresAtUtc.ShouldBe(renewAt.AddSeconds(30));

        var rejected = await store.TryRenewLeaseAsync(new TriggerLeaseRenewRequest(lease, "instance-2", renewAt), CancellationToken.None);
        rejected.ShouldBeNull();
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
        var nextLease = reacquired.ShouldHaveSingleItem();
        nextLease.TriggerId.ShouldBe(triggerKey);
        nextLease.FireAtUtc.ShouldBe(retryAt);
    }

    [Fact]
    public async Task OneOff_trigger_fires_once()
    {
        var now = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);
        var store = CreateStore(now, 30);

        var jobKey = "1:dev:samples:once";
        var triggerId = $"{jobKey}:once";

        await store.UpsertJobAsync(new JobDefinition(jobKey, "samples", "once", null, "sample", null), CancellationToken.None);
        await store.UpsertTriggerAsync(
            new TriggerDefinition(triggerId, jobKey, TriggerSchedule.OnceExpression, DefaultScope, StartAtUtc: now),
            CancellationToken.None);

        var lease = (await store.AcquireAsync(new TriggerAcquireRequest(DefaultScope, "instance-1", now, 1), CancellationToken.None))
            .ShouldHaveSingleItem();

        await store.ReleaseAsync(new TriggerReleaseRequest(lease, true, null), CancellationToken.None);

        var reacquire = await store.AcquireAsync(new TriggerAcquireRequest(DefaultScope, "instance-1", now.AddMinutes(1), 1), CancellationToken.None);
        reacquire.ShouldBeEmpty();
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

        leaseA.TriggerId.ShouldBe(jobA);
        leaseB.TriggerId.ShouldBe(jobB);

        // Wait out the lease; the trigger should be available again even without an explicit release.
        var reacquire = await store.AcquireAsync(new TriggerAcquireRequest(DefaultScope, "i3", leaseA.FireAtUtc.AddSeconds(11), 5), CancellationToken.None);
        var leaseAfterExpiry = reacquire.ShouldHaveSingleItem();
        leaseAfterExpiry.TriggerId.ShouldBe(leaseA.TriggerId);
    }

    [Fact]
    public async Task ListJobsAsync_returns_scope_matches_only()
    {
        var store = CreateStore(null, 30);
        var jobA = "1:dev:samples:list";
        var jobB = "1:qa:samples:list";

        await store.UpsertJobAsync(new JobDefinition(jobA, "samples", "list", null, null, new Dictionary<string, string> { ["owner"] = "platform" }), CancellationToken.None);
        await store.UpsertJobAsync(new JobDefinition(jobB, "samples", "list", null, null, null), CancellationToken.None);

        var results = await store.ListJobsAsync(DefaultScope, CancellationToken.None);
        results.Count.ShouldBe(1);
        results.Single().JobKey.ShouldBe(jobA);
        results.Single().Metadata!.ShouldContainKeyAndValue("owner", "platform");
    }

    [Fact]
    public async Task DeleteJobAsync_removes_job_and_triggers()
    {
        var store = CreateStore(null, 30);
        var jobKey = "1:dev:samples:delete";
        var scope = DefaultScope;

        await store.UpsertJobAsync(new JobDefinition(jobKey, "samples", "delete", null, null, null), CancellationToken.None);
        await store.UpsertTriggerAsync(new TriggerDefinition($"{jobKey}:t1", jobKey, "0 * * * * ?", scope), CancellationToken.None);
        await store.UpsertTriggerAsync(new TriggerDefinition($"{jobKey}:t2", jobKey, "*/15 * * * * ?", scope), CancellationToken.None);

        await store.DeleteJobAsync(jobKey, scope, CancellationToken.None);

        var jobs = await store.ListJobsAsync(scope, CancellationToken.None);
        jobs.ShouldBeEmpty();

        var triggers = await store.ListTriggersAsync(scope, CancellationToken.None);
        triggers.ShouldBeEmpty();
    }

    [Fact]
    public async Task DeadLetters_can_be_listed_and_resolved()
    {
        var now = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);
        var store = CreateStore(now, 30);
        var jobKey = "1:dev:samples:deadletter";
        var triggerId = $"{jobKey}:t1";

        await store.UpsertJobAsync(new JobDefinition(jobKey, "samples", "deadletter", null, null, null), CancellationToken.None);
        await store.UpsertTriggerAsync(new TriggerDefinition(triggerId, jobKey, "0 * * * * ?", DefaultScope), CancellationToken.None);

        var lease = new TriggerLease(
            LeaseId: "lease-1",
            TriggerId: triggerId,
            JobKey: jobKey,
            Scope: DefaultScope,
            FireAtUtc: now.AddMinutes(-1),
            LeaseExpiresAtUtc: now.AddMinutes(1),
            Payload: "payload");

        await store.MoveToDeadLetterAsync(
            new DeadLetterRequest(
                lease,
                Reason: "boom",
                OccurredAtUtc: now,
                Retention: TimeSpan.FromDays(1),
                Payload: "envelope",
                Metadata: new Dictionary<string, string> { ["initiator"] = "test" }),
            CancellationToken.None);

        var entries = await store.ListAsync(DefaultScope, CancellationToken.None);
        var entry = entries.ShouldHaveSingleItem();
        entry.TriggerId.ShouldBe(triggerId);
        entry.JobKey.ShouldBe(jobKey);
        entry.Reason.ShouldBe("boom");
        entry.Metadata.ShouldNotBeNull();
        entry.Metadata!.ShouldContainKeyAndValue("initiator", "test");

        var fetched = await store.FindAsync(entry.Id, DefaultScope, CancellationToken.None);
        fetched.ShouldNotBeNull();
        fetched!.Id.ShouldBe(entry.Id);

        await store.ResolveAsync(entry.Id, DefaultScope, CancellationToken.None);
        (await store.ListAsync(DefaultScope, CancellationToken.None)).ShouldBeEmpty();
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
