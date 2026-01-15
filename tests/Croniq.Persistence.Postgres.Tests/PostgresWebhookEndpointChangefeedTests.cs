using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.Postgres.Tests.Collections;
using Croniq.TestKit.Postgres;
using Croniq.TestKit.Testing;
using Shouldly;
using Microsoft.Extensions.DependencyInjection;
using Xunit;

namespace Croniq.Persistence.Postgres.Tests;

[Collection(PostgresContractTestCollection.Name)]
[Trait(TestTraits.Component, TestTraits.Components.PostgresPersistenceChangefeed)]
public sealed class PostgresWebhookEndpointChangefeedTests : IAsyncLifetime
{
    private readonly PostgresContainerFixture _sql;
    private ServiceProvider? _provider;
    private IWebhookEndpointChangefeed? _changefeed;
    private IWebhookPersistenceProvider? _webhooks;

    public PostgresWebhookEndpointChangefeedTests(PostgresContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        await PostgresDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-changefeed-tests");
        _provider = PostgresTestServiceProviderFactory.Create(_sql.ConnectionString);
        _changefeed = _provider.GetRequiredService<IWebhookEndpointChangefeed>();
        _webhooks = _provider.GetRequiredService<IWebhookPersistenceProvider>();
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
    public async Task FetchAsync_ReturnsOrderedBatches()
    {
        var scope = new PartitionScope("tenant-changefeed-tests", "dev");
        var hookKey = "tenant-changefeed-tests-dev-alpha";
        var jobKey = JobKey.Create("ops", "alpha");

        await _webhooks!.UpsertAsync(
            new WebhookEndpointUpsert(
                hookKey,
                jobKey.Value,
                scope.TenantId,
                scope.EnvironmentTag,
                Enabled: true,
                RequireSignature: true,
                RequestsPerMinute: 60,
                Secret: "secret-one",
                SignatureVersion: 1,
                Metadata: null),
            CancellationToken.None);

        await _webhooks.UpsertAsync(
            new WebhookEndpointUpsert(
                hookKey,
                jobKey.Value,
                scope.TenantId,
                scope.EnvironmentTag,
                Enabled: false,
                RequireSignature: true,
                RequestsPerMinute: 30,
                Secret: null,
                SignatureVersion: 2,
                Metadata: null),
            CancellationToken.None);

        var batch = await _changefeed!.FetchAsync(0, 10, CancellationToken.None);
        batch.Count.ShouldBe(2);
        batch.First().EventType.ShouldBe(WebhookEndpointEventTypes.Created);
        batch.Last().EventType.ShouldBe(WebhookEndpointEventTypes.Updated);

        var cursor = batch.Last().Id;
        var next = await _changefeed.FetchAsync(cursor, 10, CancellationToken.None);
        next.ShouldBeEmpty();
    }
}


