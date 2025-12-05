using System;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.Xtraq;
using Microsoft.Data.SqlClient;
using Microsoft.Extensions.DependencyInjection;
using Xunit;

namespace Croniq.Persistence.Xtraq.Tests;

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

        Assert.NotNull(lease);
        var firstFire = lease!.FireAtUtc;

        await provider.ReleaseAsync(new TriggerReleaseRequest(lease, true, null), CancellationToken.None);

        var reacquire = new TriggerAcquireRequest(scope, acquire.InstanceId, firstFire.AddMinutes(2), 10);
        var leases2 = await provider.AcquireAsync(reacquire, CancellationToken.None);
        var lease2 = leases2.FirstOrDefault(l => l.TriggerId == triggerKey);

        Assert.NotNull(lease2);
        Assert.True(lease2!.FireAtUtc > firstFire);
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

        Assert.True(deadLetters > 0, "Expected deadletter entry for failed release.");
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
}
