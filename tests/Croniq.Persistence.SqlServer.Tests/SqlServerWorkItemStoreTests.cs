using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.SqlServer;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer;
using Croniq.Persistence.SqlServer.Tests.Collections;
using Croniq.TestKit.SqlServer;
using Croniq.TestKit.Testing;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Shouldly;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests;

[Collection(SqlServerContractTestCollection.Name)]
[Trait(TestTraits.Component, TestTraits.Components.SqlPersistenceWorkItems)]
public sealed class SqlServerWorkItemStoreTests : IAsyncLifetime
{
    private readonly SqlServerContainerFixture _sql;
    private ServiceProvider? _provider;
    private IWorkItemStore? _store;
    private IDbContextFactory<SqlServerDbContext>? _dbFactory;

    public SqlServerWorkItemStoreTests(SqlServerContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-work");
        _provider = BuildServiceProvider(_sql.ConnectionString);
        _store = _provider.GetRequiredService<IWorkItemStore>();
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
    public async Task UpsertAssignmentAsync_persists_work_item_and_claim()
    {
        var scope = new PartitionScope("tenant-work", "dev");
        var assignedAt = new DateTimeOffset(2025, 1, 2, 0, 0, 0, TimeSpan.Zero);
        var executionId = Guid.NewGuid().ToString("N");
        var leaseId = Guid.NewGuid().ToString("N");
        var expiresAt = assignedAt.AddMinutes(2);

        var assignment = new WorkAssignment(
            scope,
            executionId,
            "ops:demo",
            "trigger-1",
            Attempt: 1,
            RunnerId: "runner-1",
            LeaseId: leaseId,
            LeaseExpiresAtUtc: expiresAt,
            Payload: "{\"hello\":\"world\"}",
            AssignedAtUtc: assignedAt);

        await _store!.UpsertAssignmentAsync(assignment, CancellationToken.None);

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var item = await context.WorkItems.SingleAsync(x => x.ExecutionId == executionId);
        item.TenantId.ShouldBe(scope.TenantId);
        item.EnvironmentTag.ShouldBe(scope.EnvironmentTag);
        item.JobKey.ShouldBe("ops:demo");
        item.TriggerId.ShouldBe("trigger-1");
        item.Attempt.ShouldBe(1);
        item.Status.ShouldBe(WorkItemStatus.Leased);
        item.PayloadJson.ShouldBe("{\"hello\":\"world\"}");

        var claim = await context.WorkClaims.SingleAsync(x => x.WorkItemId == item.WorkItemId);
        claim.LeaseId.ShouldBe(leaseId);
        claim.RunnerId.ShouldBe("runner-1");
        claim.LeaseExpiresAtUtc.ShouldBe(expiresAt.UtcDateTime);
        claim.LastHeartbeatAtUtc.ShouldBe(assignedAt.UtcDateTime);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task TryRenewAsync_updates_claim_deadline()
    {
        var scope = new PartitionScope("tenant-work", "dev");
        var assignedAt = new DateTimeOffset(2025, 1, 3, 0, 0, 0, TimeSpan.Zero);
        var executionId = Guid.NewGuid().ToString("N");
        var leaseId = Guid.NewGuid().ToString("N");

        await _store!.UpsertAssignmentAsync(
            new WorkAssignment(
                scope,
                executionId,
                "ops:renew",
                "trigger-renew",
                Attempt: 1,
                RunnerId: "runner-1",
                LeaseId: leaseId,
                LeaseExpiresAtUtc: assignedAt.AddMinutes(1),
                Payload: null,
                AssignedAtUtc: assignedAt),
            CancellationToken.None);

        var renewedAt = assignedAt.AddMinutes(1);
        var newExpiry = assignedAt.AddMinutes(4);
        var renewed = await _store.TryRenewAsync(
            new WorkLeaseRenewal(leaseId, "runner-1", newExpiry, renewedAt, executionId),
            CancellationToken.None);

        renewed.ShouldBeTrue();

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var claim = await context.WorkClaims.SingleAsync(x => x.LeaseId == leaseId);
        claim.LeaseExpiresAtUtc.ShouldBe(newExpiry.UtcDateTime);
        claim.LastHeartbeatAtUtc.ShouldBe(renewedAt.UtcDateTime);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task TryCompleteAsync_marks_status_and_removes_claim()
    {
        var scope = new PartitionScope("tenant-work", "dev");
        var assignedAt = new DateTimeOffset(2025, 1, 4, 0, 0, 0, TimeSpan.Zero);
        var executionId = Guid.NewGuid().ToString("N");
        var leaseId = Guid.NewGuid().ToString("N");

        await _store!.UpsertAssignmentAsync(
            new WorkAssignment(
                scope,
                executionId,
                "ops:complete",
                "trigger-complete",
                Attempt: 1,
                RunnerId: "runner-1",
                LeaseId: leaseId,
                LeaseExpiresAtUtc: assignedAt.AddMinutes(1),
                Payload: null,
                AssignedAtUtc: assignedAt),
            CancellationToken.None);

        var completedAt = assignedAt.AddMinutes(2);
        var completed = await _store.TryCompleteAsync(
            new WorkCompletion(leaseId, "runner-1", Succeeded: true, completedAt, ExecutionId: executionId),
            CancellationToken.None);

        completed.ShouldBeTrue();

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var item = await context.WorkItems.SingleAsync(x => x.ExecutionId == executionId);
        item.Status.ShouldBe(WorkItemStatus.Succeeded);
        (await context.WorkClaims.AnyAsync(x => x.LeaseId == leaseId)).ShouldBeFalse();
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
