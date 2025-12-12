using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using Croniq.Core.Jobs;
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
        var jobKey = JobKey.Create("tenant-a", "dev", "scheduler", "demo", "v1");
        var metadata = new Dictionary<string, string>
        {
            ["owner"] = "platform",
            ["team"] = "core"
        };

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, "DemoJob", jobKey.Variant, "original", metadata),
            CancellationToken.None);

        await using (var context = await _dbFactory!.CreateDbContextAsync())
        {
            var entity = await context.Jobs.SingleAsync(j => j.JobKey == jobKey.Value);
            entity.NamespaceSegment.ShouldBe(jobKey.NamespaceSegment);
            entity.Description.ShouldBe("original");
            JsonDocument.Parse(entity.MetadataJson!).RootElement.GetProperty("owner").GetString().ShouldBe("platform");
        }

        var updatedMetadata = new Dictionary<string, string> { ["owner"] = "sre" };
        await _persistence.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, "DemoJob", jobKey.Variant, "updated", updatedMetadata),
            CancellationToken.None);

        await using var reloaded = await _dbFactory.CreateDbContextAsync();
        var row = await reloaded.Jobs.SingleAsync(j => j.JobKey == jobKey.Value);
        row.Description.ShouldBe("updated");
        JsonDocument.Parse(row.MetadataJson!).RootElement.GetProperty("owner").GetString().ShouldBe("sre");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task AcquireAsync_ReturnsDueTriggerWithinScope()
    {
        var jobKey = JobKey.Create("tenant-b", "qa", "billing", "invoice");
        var scope = new PartitionScope(jobKey.TenantId, jobKey.EnvironmentTag);

        await _persistence!.UpsertJobAsync(
            new JobDefinition(jobKey.Value, jobKey.NamespaceSegment, "Invoicing", jobKey.Variant, null, null),
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

