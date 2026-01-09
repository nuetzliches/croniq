using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer;
using Croniq.Persistence.SqlServer.Tests.Collections;
using Croniq.TestKit.SqlServer;
using Croniq.TestKit.Testing;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Shouldly;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests;

[Collection(SqlServerContractTestCollection.Name)]
[Trait(TestTraits.Component, TestTraits.Components.SqlPersistenceWorkers)]
public sealed class SqlServerWorkerStoreTests : IAsyncLifetime
{
    private readonly SqlServerContainerFixture _sql;
    private ServiceProvider? _provider;
    private IWorkerStore? _workerStore;

    public SqlServerWorkerStoreTests(SqlServerContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-workers");
        _provider = BuildServiceProvider(_sql.ConnectionString);
        _workerStore = _provider.GetRequiredService<IWorkerStore>();
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
    public async Task Heartbeat_then_list_returns_online_worker()
    {
        var scope = new PartitionScope("tenant-workers", "dev");
        var seenAt = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);

        await _workerStore!.UpsertHeartbeatAsync(
            new WorkerHeartbeat(scope, "worker-1", seenAt, "{\"kind\":\"worker\"}"),
            CancellationToken.None);

        var results = await _workerStore.ListAsync(new WorkerQuery(scope, seenAt.AddSeconds(30)), CancellationToken.None);

        var worker = results.ShouldHaveSingleItem();
        worker.InstanceId.ShouldBe("worker-1");
        worker.IsOnline.ShouldBeTrue();
        worker.MetadataJson.ShouldBe("{\"kind\":\"worker\"}");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ListAsync_prunes_expired_workers()
    {
        var scope = new PartitionScope("tenant-workers", "dev");
        var seenAt = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);

        await _workerStore!.UpsertHeartbeatAsync(
            new WorkerHeartbeat(scope, "worker-old", seenAt, null),
            CancellationToken.None);

        var results = await _workerStore.ListAsync(new WorkerQuery(scope, seenAt.AddMinutes(2)), CancellationToken.None);

        results.ShouldBeEmpty();
    }

    private static ServiceProvider BuildServiceProvider(string connectionString)
    {
        var services = new ServiceCollection();
        services.AddLogging(TestLogging.Configure);
        services.Configure<WorkerStoreOptions>(options => options.OnlineTtl = TimeSpan.FromMinutes(1));
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
