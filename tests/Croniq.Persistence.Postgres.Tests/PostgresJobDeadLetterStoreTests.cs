using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.Postgres;
using Croniq.Persistence.Postgres.Tests.Collections;
using Croniq.TestKit.Postgres;
using Croniq.TestKit.Testing;
using Shouldly;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Xunit;

namespace Croniq.Persistence.Postgres.Tests;

[Collection(PostgresContractTestCollection.Name)]
[Trait(TestTraits.Component, TestTraits.Components.PostgresPersistenceDeadLetters)]
public sealed class PostgresJobDeadLetterStoreTests : IAsyncLifetime
{
    private readonly PostgresContainerFixture _sql;
    private ServiceProvider? _provider;
    private IJobPersistenceProvider? _persistence;
    private IJobDeadLetterStore? _deadLetters;

    public PostgresJobDeadLetterStoreTests(PostgresContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        await PostgresDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-deadletters");
        await PostgresDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-resolve");
        await PostgresDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-find");
        await PostgresDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-release");
        _provider = BuildServiceProvider(_sql.ConnectionString);
        _persistence = _provider.GetRequiredService<IJobPersistenceProvider>();
        _deadLetters = _provider.GetRequiredService<IJobDeadLetterStore>();
    }

    public async Task DisposeAsync()
    {
        if (_provider is IAsyncDisposable asyncDisposable)
        {
            await asyncDisposable.DisposeAsync();
        }
        else
        {
            _provider?.Dispose();
        }
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ListAsync_Filters_by_scope()
    {
        var scope = new PartitionScope("tenant-deadletters", "dev");
        var otherScope = new PartitionScope("tenant-deadletters", "qa");

        await SeedDeadLetterAsync(scope, "alpha", "t1");
        await SeedDeadLetterAsync(scope, "beta", "t2");
        await SeedDeadLetterAsync(otherScope, "gamma", "t3");

        var entries = await _deadLetters!.ListAsync(scope, CancellationToken.None);

        entries.Count().ShouldBe(2);
        entries.All(entry => entry.EnvironmentTag == scope.EnvironmentTag).ShouldBeTrue();
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ResolveAsync_Removes_entry()
    {
        var scope = new PartitionScope("tenant-resolve", "dev");
        var id = await SeedDeadLetterAsync(scope, "alpha", "t1");

        await _deadLetters!.ResolveAsync(id, scope, CancellationToken.None);

        var entry = await _deadLetters.FindAsync(id, scope, CancellationToken.None);
        entry.ShouldBeNull();
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task FindAsync_Returns_payload_and_metadata()
    {
        var scope = new PartitionScope("tenant-find", "dev");
        var id = await SeedDeadLetterAsync(scope, "alpha", "t1");

        var entry = await _deadLetters!.FindAsync(id, scope, CancellationToken.None);

        entry.ShouldNotBeNull();
        entry!.Payload.ShouldNotBeNullOrWhiteSpace();
        entry.Metadata.ShouldNotBeNull();
        entry.Metadata!.ShouldContainKeyAndValue("initiator", "test");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ReleaseAsync_Persists_deadletter_when_reason_provided()
    {
        var scope = new PartitionScope("tenant-release", "dev");
        var jobKey = JobKey.Create("ops", "release");
        var triggerId = $"{jobKey.Value}:t1";

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, jobKey.JobName, jobKey.Variant, "demo", null, AssignedRunnerId: "deadletter-test"),
            scope,
            CancellationToken.None);

        await _persistence.UpsertTriggerAsync(
            new TriggerDefinition(triggerId, jobKey.Value, "0/5 * * * * ?", scope),
            CancellationToken.None);

        var leases = await _persistence.AcquireAsync(
            new TriggerAcquireRequest(scope, "deadletter-test", DateTimeOffset.UtcNow.AddHours(1), 1),
            CancellationToken.None);
        var lease = leases.Single();

        await _persistence.ReleaseAsync(new TriggerReleaseRequest(lease, "deadletter-test", false, NextFireTimeUtc: null, DeadLetterReason: "release-failed"), CancellationToken.None);

        var entries = await _deadLetters!.ListAsync(scope, CancellationToken.None);
        entries.ShouldContain(entry => entry.TriggerId == triggerId && entry.Reason == "release-failed");
    }

    private async Task<long> SeedDeadLetterAsync(PartitionScope scope, string jobName, string triggerSuffix)
    {
        var jobKey = JobKey.Create("ops", jobName);
        var triggerId = $"{jobKey.Value}:{triggerSuffix}";

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, jobKey.JobName, jobKey.Variant, "demo", null),
            scope,
            CancellationToken.None);

        await _persistence.UpsertTriggerAsync(
            new TriggerDefinition(triggerId, jobKey.Value, "0/5 * * * * ?", scope),
            CancellationToken.None);

        var lease = new TriggerLease(
            LeaseId: Guid.NewGuid().ToString("N"),
            TriggerId: triggerId,
            JobKey: jobKey.Value,
            Scope: scope,
            FireAtUtc: DateTimeOffset.UtcNow.AddMinutes(-1),
            LeaseExpiresAtUtc: DateTimeOffset.UtcNow.AddMinutes(1),
            Payload: "payload");

        await _persistence.MoveToDeadLetterAsync(
            new DeadLetterRequest(
                lease,
                Reason: "boom",
                OccurredAtUtc: DateTimeOffset.UtcNow,
                Retention: TimeSpan.FromDays(1),
                Payload: "envelope",
                Metadata: new Dictionary<string, string> { ["initiator"] = "test" }),
            CancellationToken.None);

        var entries = await _deadLetters!.ListAsync(scope, CancellationToken.None);
        var entry = entries.Single(x => string.Equals(x.TriggerId, triggerId, StringComparison.OrdinalIgnoreCase));
        return entry.Id;
    }

    private static ServiceProvider BuildServiceProvider(string connectionString)
    {
        var services = new ServiceCollection();
        services.AddLogging(TestLogging.Configure);
        services.AddCroniqPostgresPersistence(
            sql =>
            {
                sql.ConnectionString = connectionString;
                var verboseEf = TestLogging.EnableVerboseEfDiagnostics();
                sql.EnableDetailedErrors = verboseEf;
                sql.EnableSensitiveDataLogging = verboseEf;
            });

        return services.BuildServiceProvider();
    }
}


