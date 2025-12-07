using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.Xtraq;
using Croniq.TestKit;
using FluentAssertions;
using Microsoft.Data.SqlClient;
using Microsoft.Extensions.DependencyInjection;
using Xunit;

namespace Croniq.Persistence.Xtraq.Tests;

[Trait(TestCategories.Category, TestCategories.Contract)]
public class XtraqJobPersistenceProviderTests : IClassFixture<XtraqDatabaseFixture>
{
    private readonly XtraqDatabaseFixture _fixture;

    public XtraqJobPersistenceProviderTests(XtraqDatabaseFixture fixture)
    {
        _fixture = fixture;
    }

    [Fact]
    public async Task Acquire_and_release_reschedules_trigger()
    {
        if (_fixture.SkipReason is { })
        {
            return;
        }

        var provider = CreateProvider();
        var scope = new PartitionScope("1", "dev");
        var variant = "v1";
        var jobName = $"job-{Guid.NewGuid():N}";
        var jobKey = $"1:dev:tests:{jobName}:{variant}";
        var triggerKey = jobKey;
        var instanceId = XtraqDatabaseFixture.DefaultInstanceId;

        await provider.UpsertJobAsync(
            new JobDefinition(jobKey, "tests", jobName, variant, "integration job", null),
            CancellationToken.None);

        await provider.UpsertTriggerAsync(
            new TriggerDefinition(triggerKey, jobKey, "0/1 * * * * ?", scope),
            CancellationToken.None);

        var acquire = new TriggerAcquireRequest(scope, instanceId, DateTimeOffset.UtcNow.AddMinutes(1), 10);
        var leases = await provider.AcquireAsync(acquire, CancellationToken.None);
        var lease = leases.FirstOrDefault(l => l.TriggerId == triggerKey);

        lease.Should().NotBeNull();
        var firstFire = lease!.FireAtUtc;

        await provider.ReleaseAsync(new TriggerReleaseRequest(lease, true, null), CancellationToken.None);

        var reacquire = new TriggerAcquireRequest(scope, acquire.InstanceId, firstFire.AddMinutes(2), 10);
        var leases2 = await provider.AcquireAsync(reacquire, CancellationToken.None);
        var lease2 = leases2.FirstOrDefault(l => l.TriggerId == triggerKey);

        lease2.Should().NotBeNull();
        lease2!.FireAtUtc.Should().BeAfter(firstFire);
    }

    [Fact]
    public async Task Release_with_failure_persists_deadletter()
    {
        if (_fixture.SkipReason is { })
        {
            return;
        }

        var provider = CreateProvider();
        var scope = new PartitionScope("1", "dev");
        var jobName = $"job-{Guid.NewGuid():N}";
        var variant = "v1";
        var jobKey = $"1:dev:tests:{jobName}:{variant}";
        var triggerKey = $"{jobKey}:failure";
        var instanceId = XtraqDatabaseFixture.DefaultInstanceId;

        await provider.UpsertJobAsync(
            new JobDefinition(jobKey, "tests", jobName, variant, "deadletter job", null),
            CancellationToken.None);

        await provider.UpsertTriggerAsync(
            new TriggerDefinition(triggerKey, jobKey, "0/1 * * * * ?", scope),
            CancellationToken.None);

        var lease = (await provider.AcquireAsync(
            new TriggerAcquireRequest(scope, instanceId, DateTimeOffset.UtcNow.AddMinutes(1), 5),
            CancellationToken.None)).First(l => l.TriggerId == triggerKey);

        await provider.ReleaseAsync(
            new TriggerReleaseRequest(lease, false, lease.FireAtUtc.AddMinutes(1), "boom"),
            CancellationToken.None);

        var triggerId = await GetTriggerIdAsync(triggerKey);
        var deadLetters = await CountDeadLettersAsync(triggerId, "boom");

        deadLetters.Should().BeGreaterThan(0, "a failed release should dead-letter the trigger");
    }

    [Fact]
    public async Task Release_failure_without_nextfire_nullifies_schedule()
    {
        if (_fixture.SkipReason is { })
        {
            return;
        }

        var provider = CreateProvider();
        var scope = new PartitionScope("1", "dev");
        var jobName = $"job-{Guid.NewGuid():N}";
        var jobKey = $"1:dev:tests:{jobName}";
        var triggerKey = $"{jobKey}:nofire";
        var instanceId = XtraqDatabaseFixture.DefaultInstanceId;

        await provider.UpsertJobAsync(
            new JobDefinition(jobKey, "tests", jobName, null, "deadletter no next", null),
            CancellationToken.None);

        await provider.UpsertTriggerAsync(
            new TriggerDefinition(triggerKey, jobKey, "0/1 * * * * ?", scope),
            CancellationToken.None);

        var lease = (await provider.AcquireAsync(
            new TriggerAcquireRequest(scope, instanceId, DateTimeOffset.UtcNow.AddMinutes(1), 5),
            CancellationToken.None)).First(l => l.TriggerId == triggerKey);

        await provider.ReleaseAsync(
            new TriggerReleaseRequest(lease, false, null, "fail-no-next"),
            CancellationToken.None);

        var persisted = await GetNextFireAtAsync(triggerKey);
        persisted.Should().BeNull();
    }

    [Fact]
    public async Task Release_updates_next_fire_when_provided()
    {
        if (_fixture.SkipReason is { })
        {
            return;
        }

        var provider = CreateProvider();
        var scope = new PartitionScope("1", "dev");
        var jobName = $"job-{Guid.NewGuid():N}";
        var jobKey = $"1:dev:tests:{jobName}";
        var triggerKey = $"{jobKey}:nextfire";
        var instanceId = XtraqDatabaseFixture.DefaultInstanceId;

        await provider.UpsertJobAsync(
            new JobDefinition(jobKey, "tests", jobName, null, "nextfire job", null),
            CancellationToken.None);

        await provider.UpsertTriggerAsync(
            new TriggerDefinition(triggerKey, jobKey, "0/1 * * * * ?", scope),
            CancellationToken.None);

        var lease = (await provider.AcquireAsync(
            new TriggerAcquireRequest(scope, instanceId, DateTimeOffset.UtcNow.AddMinutes(1), 5),
            CancellationToken.None)).First(l => l.TriggerId == triggerKey);

        var nextFire = lease.FireAtUtc.AddMinutes(2);
        await provider.ReleaseAsync(
            new TriggerReleaseRequest(lease, true, nextFire),
            CancellationToken.None);

        var persisted = await GetNextFireAtAsync(triggerKey);
        persisted.Should().Be(nextFire.UtcDateTime);
    }

    [Fact]
    public async Task Release_success_sets_succeeded_flag()
    {
        if (_fixture.SkipReason is { })
        {
            return;
        }

        var provider = CreateProvider();
        var scope = new PartitionScope("1", "dev");
        var jobName = $"job-{Guid.NewGuid():N}";
        var jobKey = $"1:dev:tests:{jobName}";
        var triggerKey = $"{jobKey}:successflag";
        var instanceId = XtraqDatabaseFixture.DefaultInstanceId;

        await provider.UpsertJobAsync(
            new JobDefinition(jobKey, "tests", jobName, null, "success flag job", null),
            CancellationToken.None);

        await provider.UpsertTriggerAsync(
            new TriggerDefinition(triggerKey, jobKey, "0/1 * * * * ?", scope),
            CancellationToken.None);

        var lease = (await provider.AcquireAsync(
            new TriggerAcquireRequest(scope, instanceId, DateTimeOffset.UtcNow.AddMinutes(1), 5),
            CancellationToken.None)).First(l => l.TriggerId == triggerKey);

        await provider.ReleaseAsync(
            new TriggerReleaseRequest(lease, true, lease.FireAtUtc.AddMinutes(1)),
            CancellationToken.None);

        var succeeded = await GetSucceededFlagAsync(lease.LeaseId);
        succeeded.Should().BeTrue();
    }

    [Fact]
    public async Task Release_failure_sets_deadletter_reason()
    {
        if (_fixture.SkipReason is { })
        {
            return;
        }

        var provider = CreateProvider();
        var scope = new PartitionScope("1", "dev");
        var jobName = $"job-{Guid.NewGuid():N}";
        var jobKey = $"1:dev:tests:{jobName}";
        var triggerKey = $"{jobKey}:deadletterflag";
        var instanceId = XtraqDatabaseFixture.DefaultInstanceId;

        await provider.UpsertJobAsync(
            new JobDefinition(jobKey, "tests", jobName, null, "deadletter flag job", null),
            CancellationToken.None);

        await provider.UpsertTriggerAsync(
            new TriggerDefinition(triggerKey, jobKey, "0/1 * * * * ?", scope),
            CancellationToken.None);

        var lease = (await provider.AcquireAsync(
            new TriggerAcquireRequest(scope, instanceId, DateTimeOffset.UtcNow.AddMinutes(1), 5),
            CancellationToken.None)).First(l => l.TriggerId == triggerKey);

        const string reason = "flagged-error";
        await provider.ReleaseAsync(
            new TriggerReleaseRequest(lease, false, lease.FireAtUtc.AddMinutes(1), reason),
            CancellationToken.None);

        var deadLetters = await CountDeadLettersAsync(await GetTriggerIdAsync(triggerKey), reason);
        deadLetters.Should().BeGreaterThan(0);
        var releasedCount = await GetReleasedCountAsync(triggerKey);
        releasedCount.Should().Be(1);
    }

    [Fact]
    public async Task Move_to_deadletter_persists_payload()
    {
        if (_fixture.SkipReason is { })
        {
            return;
        }

        var provider = CreateProvider();
        var scope = new PartitionScope("1", "dev");
        var jobName = $"job-{Guid.NewGuid():N}";
        var jobKey = $"1:dev:tests:{jobName}";
        var triggerKey = $"{jobKey}:dlq";
        var instanceId = XtraqDatabaseFixture.DefaultInstanceId;

        await provider.UpsertJobAsync(
            new JobDefinition(jobKey, "tests", jobName, null, "deadletter api job", null),
            CancellationToken.None);

        await provider.UpsertTriggerAsync(
            new TriggerDefinition(triggerKey, jobKey, "0/1 * * * * ?", scope),
            CancellationToken.None);

        var lease = (await provider.AcquireAsync(
            new TriggerAcquireRequest(scope, instanceId, DateTimeOffset.UtcNow.AddMinutes(1), 5),
            CancellationToken.None)).First(l => l.TriggerId == triggerKey);

        var metadata = new Dictionary<string, string>
        {
            ["exception.type"] = typeof(InvalidOperationException).FullName!,
            ["policy.retryAttempts"] = "3"
        };

        var request = new DeadLetterRequest(
            lease,
            "policy-deadletter",
            DateTimeOffset.UtcNow,
            TimeSpan.FromDays(5),
            "{\"custom\":\"payload\"}",
            metadata);

        await provider.MoveToDeadLetterAsync(request, CancellationToken.None);

        var triggerId = await GetTriggerIdAsync(triggerKey);
        var deadLetters = await CountDeadLettersAsync(triggerId, "policy-deadletter");
        deadLetters.Should().BeGreaterThan(0);

        await provider.ReleaseAsync(new TriggerReleaseRequest(lease, false, null), CancellationToken.None);
    }

    private IJobPersistenceProvider CreateProvider()
    {
        if (_fixture.SkipReason is { } reason)
        {
            throw new InvalidOperationException(reason);
        }

        return _fixture.CreateProvider().GetRequiredService<IJobPersistenceProvider>();
    }

    private async Task<long> GetTriggerIdAsync(string triggerKey)
    {
        if (_fixture.ConnectionString is null)
        {
            throw new InvalidOperationException("Connection string is not configured.");
        }

        await using var conn = new SqlConnection(_fixture.ConnectionString);
        await conn.OpenAsync();
        await using var cmd = new SqlCommand("SELECT TOP(1) TriggerId FROM croniq.Triggers WHERE TriggerKey = @k", conn);
        cmd.Parameters.AddWithValue("@k", triggerKey);
        var result = await cmd.ExecuteScalarAsync();
        return result switch
        {
            long l => l,
            int i => i,
            decimal d => (long)d,
            _ => throw new InvalidOperationException($"Trigger '{triggerKey}' not found.")
        };
    }

    private async Task<int> CountDeadLettersAsync(long triggerId, string reason)
    {
        if (_fixture.ConnectionString is null)
        {
            throw new InvalidOperationException("Connection string is not configured.");
        }

        await using var conn = new SqlConnection(_fixture.ConnectionString);
        await conn.OpenAsync();
        await using var cmd = new SqlCommand("SELECT COUNT(*) FROM croniq.TriggerDeadLetter WHERE TriggerId = @id AND DeadLetterReason = @r", conn);
        cmd.Parameters.AddWithValue("@id", triggerId);
        cmd.Parameters.AddWithValue("@r", reason);
        var result = await cmd.ExecuteScalarAsync();
        return Convert.ToInt32(result, System.Globalization.CultureInfo.InvariantCulture);
    }

    private async Task<DateTime?> GetNextFireAtAsync(string triggerKey)
    {
        if (_fixture.ConnectionString is null)
        {
            throw new InvalidOperationException("Connection string is not configured.");
        }

        await using var conn = new SqlConnection(_fixture.ConnectionString);
        await conn.OpenAsync();
        await using var cmd = new SqlCommand("SELECT TOP(1) NextFireAtUtc FROM croniq.Triggers WHERE TriggerKey = @k", conn);
        cmd.Parameters.AddWithValue("@k", triggerKey);
        var result = await cmd.ExecuteScalarAsync();
        return result as DateTime?;
    }

    private async Task<bool> GetSucceededFlagAsync(string leaseId)
    {
        if (_fixture.ConnectionString is null)
        {
            throw new InvalidOperationException("Connection string is not configured.");
        }

        await using var conn = new SqlConnection(_fixture.ConnectionString);
        await conn.OpenAsync().ConfigureAwait(false);
        await using var cmd = new SqlCommand("SELECT TOP(1) Succeeded FROM croniq.TriggerLeases WHERE LeaseId = @id ORDER BY LeaseId DESC", conn);
        cmd.Parameters.AddWithValue("@id", long.Parse(leaseId, System.Globalization.CultureInfo.InvariantCulture));
        var result = await cmd.ExecuteScalarAsync().ConfigureAwait(false);
        return result is int i && i == 1;
    }

    private async Task<int> GetReleasedCountAsync(string triggerKey)
    {
        if (_fixture.ConnectionString is null)
        {
            throw new InvalidOperationException("Connection string is not configured.");
        }

        await using var conn = new SqlConnection(_fixture.ConnectionString);
        await conn.OpenAsync().ConfigureAwait(false);
        await using var cmd = new SqlCommand("SELECT COUNT(*) FROM croniq.TriggerLeases WHERE TriggerKey = @k AND ReleasedAtUtc IS NOT NULL", conn);
        cmd.Parameters.AddWithValue("@k", triggerKey);
        var result = await cmd.ExecuteScalarAsync().ConfigureAwait(false);
        return Convert.ToInt32(result, System.Globalization.CultureInfo.InvariantCulture);
    }
}
