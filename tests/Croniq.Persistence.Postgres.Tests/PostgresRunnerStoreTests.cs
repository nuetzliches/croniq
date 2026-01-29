using System;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.Postgres;
using Croniq.Persistence.Postgres.Tests.Collections;
using Croniq.TestKit.Postgres;
using Croniq.TestKit.Testing;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Shouldly;
using Xunit;

namespace Croniq.Persistence.Postgres.Tests;

[Collection(PostgresContractTestCollection.Name)]
[Trait(TestTraits.Component, TestTraits.Components.PostgresPersistenceRunners)]
public sealed class PostgresRunnerStoreTests : IAsyncLifetime
{
    private readonly PostgresContainerFixture _sql;
    private ServiceProvider? _provider;
    private IRunnerStore? _runnerStore;

    public PostgresRunnerStoreTests(PostgresContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        await PostgresDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-runners");
        _provider = BuildServiceProvider(_sql.ConnectionString);
        _runnerStore = _provider.GetRequiredService<IRunnerStore>();
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
    public async Task Heartbeat_then_list_returns_online_runner()
    {
        var scope = new PartitionScope("tenant-runners", "dev");
        var seenAt = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);

        await _runnerStore!.UpsertHeartbeatAsync(
            new RunnerHeartbeat(scope, "runner-1", seenAt, "{\"kind\":\"http\"}"),
            CancellationToken.None);

        var results = await _runnerStore.ListAsync(new RunnerQuery(scope, seenAt.AddSeconds(30)), CancellationToken.None);

        var runner = results.ShouldHaveSingleItem();
        runner.RunnerId.ShouldBe("runner-1");
        runner.IsOnline.ShouldBeTrue();
        runner.MetadataJson.ShouldBe("{\"kind\":\"http\"}");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ListAsync_prunes_expired_runners()
    {
        var scope = new PartitionScope("tenant-runners", "dev");
        var seenAt = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);

        await _runnerStore!.UpsertHeartbeatAsync(
            new RunnerHeartbeat(scope, "runner-old", seenAt, null),
            CancellationToken.None);

        var results = await _runnerStore.ListAsync(new RunnerQuery(scope, seenAt.AddMinutes(20)), CancellationToken.None);

        results.ShouldBeEmpty();
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ListAsync_includeOffline_returns_offline_runner()
    {
        var scope = new PartitionScope("tenant-runners", "dev");
        var seenAt = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);

        await _runnerStore!.UpsertHeartbeatAsync(
            new RunnerHeartbeat(scope, "runner-offline", seenAt, null),
            CancellationToken.None);

        var results = await _runnerStore.ListAsync(
            new RunnerQuery(scope, seenAt.AddMinutes(2), IncludeOffline: true),
            CancellationToken.None);

        var runner = results.ShouldHaveSingleItem();
        runner.RunnerId.ShouldBe("runner-offline");
        runner.IsOnline.ShouldBeFalse();
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task DeleteAsync_removes_runner()
    {
        var scope = new PartitionScope("tenant-runners", "dev");
        var seenAt = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);

        await _runnerStore!.UpsertHeartbeatAsync(
            new RunnerHeartbeat(scope, "runner-remove", seenAt, null),
            CancellationToken.None);

        var removed = await _runnerStore.DeleteAsync(
            new RunnerLookup(scope, "runner-remove", seenAt.AddSeconds(10)),
            CancellationToken.None);

        removed.ShouldBeTrue();

        var results = await _runnerStore.ListAsync(
            new RunnerQuery(scope, seenAt.AddSeconds(10), IncludeOffline: true),
            CancellationToken.None);

        results.ShouldBeEmpty();
    }

    private static ServiceProvider BuildServiceProvider(string connectionString)
    {
        var services = new ServiceCollection();
        services.AddLogging(TestLogging.Configure);
        services.Configure<RunnerStoreOptions>(options =>
        {
            options.OnlineTtl = TimeSpan.FromMinutes(1);
            options.OfflineRetentionTtl = TimeSpan.FromMinutes(10);
        });
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


