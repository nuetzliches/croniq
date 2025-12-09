using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer.Tests.Collections;
using Croniq.TestKit.SqlServer;
using Croniq.TestKit.Testing;
using FluentAssertions;
using Microsoft.Extensions.DependencyInjection;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests;

[Collection(SqlServerContractTestCollection.Name)]
public sealed class SqlServerWebhookEndpointChangefeedTests : IAsyncLifetime
{
    private readonly SqlServerContainerFixture _sql;
    private ServiceProvider? _provider;
    private IWebhookEndpointChangefeed? _changefeed;
    private IWebhookPersistenceProvider? _webhooks;

    public SqlServerWebhookEndpointChangefeedTests(SqlServerContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        _provider = SqlServerTestServiceProviderFactory.Create(_sql.ConnectionString);
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
        var jobKey = JobKey.Create(scope.TenantId, scope.EnvironmentTag, "ops", "alpha");

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
        batch.Should().HaveCount(2);
        batch.First().EventType.Should().Be(WebhookEndpointEventTypes.Created);
        batch.Last().EventType.Should().Be(WebhookEndpointEventTypes.Updated);

        var cursor = batch.Last().Id;
        var next = await _changefeed.FetchAsync(cursor, 10, CancellationToken.None);
        next.Should().BeEmpty();
    }
}
