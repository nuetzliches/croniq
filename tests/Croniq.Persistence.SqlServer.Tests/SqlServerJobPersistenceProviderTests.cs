using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using Croniq.Core.Jobs;
using Croniq.Core.Scheduling;
using Croniq.Data.SqlServer;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer;
using Croniq.Persistence.SqlServer.Tests.Collections;
using Croniq.TestKit.SqlServer;
using Croniq.TestKit.Testing;
using Shouldly;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests;

[Collection(SqlServerContractTestCollection.Name)]
[Trait(TestTraits.Component, TestTraits.Components.SqlPersistenceJobs)]
public sealed class SqlServerJobPersistenceProviderTests : IAsyncLifetime
{
    private readonly SqlServerContainerFixture _sql;
    private ServiceProvider? _provider;
    private IJobPersistenceProvider? _persistence;
    private IDbContextFactory<SqlServerDbContext>? _dbFactory;

    public SqlServerJobPersistenceProviderTests(SqlServerContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-a");
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-b");
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-c");
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-d");
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-tz");
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-renew");
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-once");
        _provider = BuildServiceProvider(_sql.ConnectionString);
        _persistence = _provider.GetRequiredService<IJobPersistenceProvider>();
        _dbFactory = _provider.GetRequiredService<IDbContextFactory<SqlServerDbContext>>();
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
    public async Task UpsertJobAsync_PersistsMetadata()
    {
        var scope = new PartitionScope("tenant-a", "dev");
        var jobKey = JobKey.Create("scheduler", "demo", "v1");
        var metadata = new Dictionary<string, string>
        {
            ["owner"] = "platform",
            ["team"] = "core"
        };

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, "DemoJob", jobKey.Variant, "original", metadata),
            scope,
            CancellationToken.None);

        await using (var context = await _dbFactory!.CreateDbContextAsync())
        {
            var entity = await context.Jobs.SingleAsync(j =>
                j.TenantId == scope.TenantId && j.EnvironmentTag == scope.EnvironmentTag && j.JobKey == jobKey.Value);
            entity.NamespaceSegment.ShouldBe(jobKey.NamespaceSegment);
            entity.Description.ShouldBe("original");
            JsonDocument.Parse(entity.MetadataJson!).RootElement.GetProperty("owner").GetString().ShouldBe("platform");
        }

        var updatedMetadata = new Dictionary<string, string> { ["owner"] = "sre" };
        await _persistence.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, "DemoJob", jobKey.Variant, "updated", updatedMetadata),
            scope,
            CancellationToken.None);

        await using var reloaded = await _dbFactory.CreateDbContextAsync();
        var row = await reloaded.Jobs.SingleAsync(j =>
            j.TenantId == scope.TenantId && j.EnvironmentTag == scope.EnvironmentTag && j.JobKey == jobKey.Value);
        row.Description.ShouldBe("updated");
        JsonDocument.Parse(row.MetadataJson!).RootElement.GetProperty("owner").GetString().ShouldBe("sre");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ListJobsAsync_Returns_matches_for_scope()
    {
        var jobKey = JobKey.Create("ops", "notify");
        var scope = new PartitionScope("tenant-c", "dev");
        var otherScope = new PartitionScope("tenant-c", "qa");

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, jobKey.JobName, jobKey.Variant, "dev job", null),
            scope,
            CancellationToken.None);
        await _persistence.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, jobKey.JobName, jobKey.Variant, "qa job", null),
            otherScope,
            CancellationToken.None);
        var jobs = await _persistence.ListJobsAsync(scope, CancellationToken.None);

        jobs.Count.ShouldBe(1);
        jobs.Single().JobKey.ShouldBe(jobKey.Value);
        jobs.Single().Description.ShouldBe("dev job");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task DeleteJobAsync_Removes_job_and_triggers()
    {
        var scope = new PartitionScope("tenant-d", "dev");
        var jobKey = JobKey.Create("billing", "cleanup");

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, jobKey.JobName, jobKey.Variant, null, null),
            scope,
            CancellationToken.None);
        var trigger = new TriggerDefinition($"{jobKey.Value}:nightly", jobKey.Value, "0 0 * * * ?", scope);
        await _persistence.UpsertTriggerAsync(trigger, CancellationToken.None);

        await _persistence.DeleteJobAsync(jobKey.Value, scope, CancellationToken.None);

        await using (var context = await _dbFactory!.CreateDbContextAsync())
        {
            (await context.Jobs.AnyAsync(j => j.JobKey == jobKey.Value)).ShouldBeFalse();
            (await context.Triggers.AnyAsync(t => t.JobKey == jobKey.Value)).ShouldBeFalse();
        }
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task UpsertTriggerAsync_Persists_time_zone_id()
    {
        var scope = new PartitionScope("tenant-tz", "dev");
        var jobKey = JobKey.Create("ops", "clock");

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, jobKey.JobName, jobKey.Variant, "demo", null),
            scope,
            CancellationToken.None);

        var trigger = new TriggerDefinition($"{jobKey.Value}:tz", jobKey.Value, "0 0 * * * ?", scope, TimeZoneId: "UTC");
        await _persistence.UpsertTriggerAsync(trigger, CancellationToken.None);

        var triggers = await _persistence.ListTriggersAsync(scope, CancellationToken.None);
        triggers.Single().TimeZoneId.ShouldBe("UTC");

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var entity = await context.Triggers.SingleAsync(t => t.TriggerKey == trigger.TriggerId);
        entity.TimeZoneId.ShouldBe("UTC");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task AcquireAsync_ReturnsDueTriggerWithinScope()
    {
        var scope = new PartitionScope("tenant-b", "qa");
        var jobKey = JobKey.Create("billing", "invoice");

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, "Invoicing", jobKey.Variant, null, null),
            scope,
            CancellationToken.None);

        var trigger = new TriggerDefinition(
            TriggerId: $"{jobKey.Value}:trigger",
            JobKey: jobKey.Value,
            ScheduleExpression: "0/5 * * * * ?",
            Scope: scope,
            Enabled: true,
            Metadata: new Dictionary<string, string> { ["kind"] = "cron" });

        await _persistence.UpsertTriggerAsync(trigger, CancellationToken.None);

        await using (var context = await _dbFactory!.CreateDbContextAsync())
        {
            var entity = await context.Triggers.SingleAsync(t => t.TriggerKey == trigger.TriggerId);
            entity.NextFireAtUtc = DateTime.UtcNow.AddSeconds(-30);
            entity.Enabled = true;
            await context.SaveChangesAsync();
        }

        var request = new TriggerAcquireRequest(scope, "instance-1", DateTimeOffset.UtcNow, BatchSize: 5);
        var leases = await _persistence.AcquireAsync(request, CancellationToken.None);

        leases.Count().ShouldBe(1);
        var lease = leases.Single();
        lease.JobKey.ShouldBe(jobKey.Value);
        lease.TriggerId.ShouldBe(trigger.TriggerId);
        lease.Scope.ShouldBe(scope);
        lease.LeaseExpiresAtUtc.ShouldBeGreaterThan(lease.FireAtUtc);

        await using var verification = await _dbFactory.CreateDbContextAsync();
        var row = await verification.Triggers.SingleAsync(t => t.TriggerKey == trigger.TriggerId);
        row.LeaseId.ShouldBe(lease.LeaseId);
        row.LeaseInstanceId.ShouldBe("instance-1");
        row.LeaseExpiresAtUtc.ShouldNotBeNull();
        row.LeaseExpiresAtUtc!.Value.ShouldBeGreaterThan(DateTime.UtcNow);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task TryRenewLeaseAsync_ExtendsLease()
    {
        var scope = new PartitionScope("tenant-renew", "dev");
        var jobKey = JobKey.Create("billing", "renew");

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, "RenewJob", jobKey.Variant, null, null),
            scope,
            CancellationToken.None);

        var trigger = new TriggerDefinition(
            TriggerId: $"{jobKey.Value}:trigger",
            JobKey: jobKey.Value,
            ScheduleExpression: "0/5 * * * * ?",
            Scope: scope,
            Enabled: true);

        await _persistence.UpsertTriggerAsync(trigger, CancellationToken.None);

        await using (var context = await _dbFactory!.CreateDbContextAsync())
        {
            var entity = await context.Triggers.SingleAsync(t => t.TriggerKey == trigger.TriggerId);
            entity.NextFireAtUtc = DateTime.UtcNow.AddSeconds(-30);
            entity.Enabled = true;
            await context.SaveChangesAsync();
        }

        var lease = (await _persistence.AcquireAsync(new TriggerAcquireRequest(scope, "instance-1", DateTimeOffset.UtcNow, BatchSize: 1), CancellationToken.None))
            .Single();

        var renewed = await _persistence.TryRenewLeaseAsync(
            new TriggerLeaseRenewRequest(lease, "instance-1", DateTimeOffset.UtcNow),
            CancellationToken.None);

        renewed.ShouldNotBeNull();
        renewed!.LeaseExpiresAtUtc.ShouldBeGreaterThan(lease.LeaseExpiresAtUtc);

        await using var verification = await _dbFactory.CreateDbContextAsync();
        var row = await verification.Triggers.SingleAsync(t => t.TriggerKey == trigger.TriggerId);
        row.LeaseExpiresAtUtc.ShouldNotBeNull();
        row.LeaseExpiresAtUtc!.Value.ShouldBeGreaterThan(lease.LeaseExpiresAtUtc.UtcDateTime);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ReleaseAsync_Rejects_instance_mismatch()
    {
        var scope = new PartitionScope("tenant-renew", "dev");
        var jobKey = JobKey.Create("billing", "release-guard");

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, jobKey.JobName, jobKey.Variant, null, null),
            scope,
            CancellationToken.None);

        var trigger = new TriggerDefinition(
            TriggerId: $"{jobKey.Value}:trigger",
            JobKey: jobKey.Value,
            ScheduleExpression: "0/5 * * * * ?",
            Scope: scope,
            Enabled: true);

        await _persistence.UpsertTriggerAsync(trigger, CancellationToken.None);

        await using (var context = await _dbFactory!.CreateDbContextAsync())
        {
            var entity = await context.Triggers.SingleAsync(t => t.TriggerKey == trigger.TriggerId);
            entity.NextFireAtUtc = DateTime.UtcNow.AddSeconds(-30);
            entity.Enabled = true;
            await context.SaveChangesAsync();
        }

        var lease = (await _persistence.AcquireAsync(
            new TriggerAcquireRequest(scope, "instance-1", DateTimeOffset.UtcNow, BatchSize: 1),
            CancellationToken.None)).Single();

        await Should.ThrowAsync<InvalidOperationException>(() =>
            _persistence.ReleaseAsync(new TriggerReleaseRequest(lease, "instance-2", true, null), CancellationToken.None));
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task UpsertTriggerAsync_AllowsOnceExpression()
    {
        var scope = new PartitionScope("tenant-once", "dev");
        var jobKey = JobKey.Create("ops", "once");

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, jobKey.JobName, jobKey.Variant, null, null),
            scope,
            CancellationToken.None);

        var startAt = DateTimeOffset.UtcNow.AddMinutes(5);
        startAt = new DateTimeOffset(startAt.Year, startAt.Month, startAt.Day, startAt.Hour, startAt.Minute, startAt.Second, TimeSpan.Zero);

        var trigger = new TriggerDefinition(
            TriggerId: "once-trigger",
            JobKey: jobKey.Value,
            ScheduleExpression: TriggerSchedule.OnceExpression,
            Scope: scope,
            StartAtUtc: startAt);

        await _persistence.UpsertTriggerAsync(trigger, CancellationToken.None);

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var row = await context.Triggers.SingleAsync(t => t.TriggerKey == trigger.TriggerId);
        row.CronExpression.ShouldBe(TriggerSchedule.OnceExpression);
        row.NextFireAtUtc.ShouldNotBeNull();
        row.NextFireAtUtc!.Value.ShouldBe(startAt.UtcDateTime);
    }

    private static ServiceProvider BuildServiceProvider(string connectionString)
    {
        var services = new ServiceCollection();
        services.AddLogging(TestLogging.Configure);
        services.AddCroniqSqlServerPersistence(
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

