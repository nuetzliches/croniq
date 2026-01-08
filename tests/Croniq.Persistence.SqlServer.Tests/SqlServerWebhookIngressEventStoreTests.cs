using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.SqlServer;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer.Tests.Collections;
using Croniq.TestKit.SqlServer;
using Croniq.TestKit.Testing;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Shouldly;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests;

[Collection(SqlServerContractTestCollection.Name)]
[Trait(TestTraits.Component, TestTraits.Components.SqlPersistenceWebhooks)]
public sealed class SqlServerWebhookIngressEventStoreTests : IAsyncLifetime
{
    private readonly SqlServerContainerFixture _sql;
    private ServiceProvider? _provider;
    private IWebhookIngressEventStore? _store;
    private IDbContextFactory<SqlServerDbContext>? _dbFactory;

    public SqlServerWebhookIngressEventStoreTests(SqlServerContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, "tenant-ingress");
        _provider = SqlServerTestServiceProviderFactory.Create(_sql.ConnectionString);
        _store = _provider.GetRequiredService<IWebhookIngressEventStore>();
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
    public async Task EnqueueAsync_persists_webhook_event()
    {
        var receivedAt = new DateTimeOffset(2025, 1, 2, 0, 0, 0, TimeSpan.Zero);
        var request = new WebhookIngressEventCreate(
            EventId: Guid.NewGuid().ToString("N"),
            HookKey: "invoice-paid",
            JobKey: "samples:logging-job",
            TenantId: "tenant-ingress",
            EnvironmentTag: "dev",
            Payload: "{\"hello\":\"world\"}",
            Headers: new Dictionary<string, string> { ["x-test"] = "header" },
            Metadata: new Dictionary<string, string> { ["source"] = "sample" },
            ReceivedAtUtc: receivedAt);

        await _store!.EnqueueAsync(request, CancellationToken.None);

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var entity = await context.WebhookIngressEvents.SingleAsync(x => x.EventId == request.EventId);
        entity.HookKey.ShouldBe("invoice-paid");
        entity.JobKey.ShouldBe("samples:logging-job");
        entity.TenantId.ShouldBe("tenant-ingress");
        entity.EnvironmentTag.ShouldBe("dev");
        entity.Payload.ShouldBe("{\"hello\":\"world\"}");
        entity.HeadersJson.ShouldContain("x-test");
        entity.MetadataJson.ShouldContain("sample");
        entity.Status.ShouldBe("Pending");
        entity.AttemptCount.ShouldBe(0);
        entity.LeaseId.ShouldBeNull();
        entity.LeaseExpiresAtUtc.ShouldBeNull();
        entity.ReceivedAtUtc.ShouldBe(receivedAt.UtcDateTime);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task AcquireAsync_assigns_lease_and_updates_status()
    {
        var now = new DateTimeOffset(2025, 1, 3, 0, 0, 0, TimeSpan.Zero);
        var request = new WebhookIngressEventCreate(
            EventId: Guid.NewGuid().ToString("N"),
            HookKey: "hook-lease",
            JobKey: "samples:lease",
            TenantId: "tenant-ingress",
            EnvironmentTag: "dev",
            Payload: "{}",
            Headers: null,
            Metadata: null,
            ReceivedAtUtc: now);

        await _store!.EnqueueAsync(request, CancellationToken.None);

        var leases = await _store.AcquireAsync(
            new WebhookIngressAcquireRequest(
                new PartitionScope("tenant-ingress", "dev"),
                now,
                MaxCount: 10,
                LeaseDuration: TimeSpan.FromSeconds(30)),
            CancellationToken.None);

        leases.Count.ShouldBe(1);
        leases.ShouldContain(lease => lease.EventId == request.EventId);

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var entity = await context.WebhookIngressEvents.SingleAsync(x => x.EventId == request.EventId);
        entity.Status.ShouldBe("Leased");
        entity.LeaseId.ShouldNotBeNull();
        entity.LeaseExpiresAtUtc.ShouldBe(now.AddSeconds(30).UtcDateTime);
        entity.AttemptCount.ShouldBe(1);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task NackAsync_requeues_event_and_clears_lease()
    {
        var now = new DateTimeOffset(2025, 1, 4, 0, 0, 0, TimeSpan.Zero);
        var request = new WebhookIngressEventCreate(
            EventId: Guid.NewGuid().ToString("N"),
            HookKey: "hook-requeue",
            JobKey: "samples:requeue",
            TenantId: "tenant-ingress",
            EnvironmentTag: "dev",
            Payload: "{}",
            Headers: null,
            Metadata: null,
            ReceivedAtUtc: now);

        await _store!.EnqueueAsync(request, CancellationToken.None);

        var leases = await _store.AcquireAsync(
            new WebhookIngressAcquireRequest(
                new PartitionScope("tenant-ingress", "dev"),
                now,
                MaxCount: 1,
                LeaseDuration: TimeSpan.FromSeconds(20)),
            CancellationToken.None);

        var lease = leases.ShouldHaveSingleItem();

        await _store.NackAsync(
            new WebhookIngressNack(
                lease.EventId,
                lease.LeaseId,
                Reason: "transient",
                NackedAtUtc: now.AddSeconds(2)),
            CancellationToken.None);

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var entity = await context.WebhookIngressEvents.SingleAsync(x => x.EventId == request.EventId);
        entity.Status.ShouldBe("Pending");
        entity.LeaseId.ShouldBeNull();
        entity.LeaseExpiresAtUtc.ShouldBeNull();
        entity.LastError.ShouldBe("transient");
    }
}
