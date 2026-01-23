using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.SqlServer;
using Croniq.Data.SqlServer.Entities;
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
public sealed class SqlServerWebhookActivityStoreTests : IAsyncLifetime
{
    private const string TenantId = "tenant-activity";
    private const string EnvironmentTag = "dev";
    private readonly SqlServerContainerFixture _sql;
    private ServiceProvider? _provider;
    private IWebhookActivityStore? _store;
    private IWebhookActivityRecorder? _recorder;
    private IDbContextFactory<SqlServerDbContext>? _dbFactory;

    public SqlServerWebhookActivityStoreTests(SqlServerContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        await SqlServerDatabaseMigrator.EnsureTenantExistsAsync(_sql.ConnectionString, TenantId);
        _provider = SqlServerTestServiceProviderFactory.Create(_sql.ConnectionString);
        _store = _provider.GetRequiredService<IWebhookActivityStore>();
        _recorder = _provider.GetRequiredService<IWebhookActivityRecorder>();
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
    public async Task ListAsync_returns_combined_activity()
    {
        var deliveredAt = new DateTimeOffset(2025, 1, 2, 10, 0, 0, TimeSpan.Zero);
        var failedAt = new DateTimeOffset(2025, 1, 2, 10, 30, 0, TimeSpan.Zero);
        var deadLetterAt = new DateTimeOffset(2025, 1, 2, 11, 0, 0, TimeSpan.Zero);

        await SeedActivityAsync(deliveredAt, failedAt, deadLetterAt);

        var entries = await _store!.ListAsync(
            new PartitionScope(TenantId, EnvironmentTag),
            new WebhookActivityQuery
            {
                FromUtc = deliveredAt.AddMinutes(-5),
                ToUtc = deadLetterAt.AddMinutes(5),
                Limit = 10
            },
            CancellationToken.None);

        entries.Count.ShouldBe(3);
        entries.ShouldContain(entry => entry.Kind == WebhookActivityKind.Delivery
            && entry.Status == WebhookActivityStatus.Warning);
        entries.ShouldContain(entry => entry.Kind == WebhookActivityKind.Delivery
            && entry.Status == WebhookActivityStatus.Failed
            && entry.Reason == "boom");
        entries.ShouldContain(entry => entry.Kind == WebhookActivityKind.DeadLetter
            && entry.Status == WebhookActivityStatus.Failed
            && entry.DeadLetterId.HasValue);

        var ordered = entries.OrderByDescending(entry => entry.OccurredAtUtc).ToArray();
        entries.ToArray().ShouldBe(ordered);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task SummarizeAsync_groups_activity_into_buckets()
    {
        var deliveredAt = new DateTimeOffset(2025, 1, 2, 10, 0, 0, TimeSpan.Zero);
        var failedAt = new DateTimeOffset(2025, 1, 2, 10, 30, 0, TimeSpan.Zero);
        var deadLetterAt = new DateTimeOffset(2025, 1, 2, 11, 0, 0, TimeSpan.Zero);

        await SeedActivityAsync(deliveredAt, failedAt, deadLetterAt);

        var summary = await _store!.SummarizeAsync(
            new PartitionScope(TenantId, EnvironmentTag),
            new WebhookActivitySummaryQuery
            {
                FromUtc = deliveredAt,
                ToUtc = deadLetterAt.AddHours(1),
                BucketMinutes = 60
            },
            CancellationToken.None);

        summary.BucketMinutes.ShouldBe(60);
        summary.Buckets.Count.ShouldBe(2);

        var buckets = summary.Buckets.ToArray();
        buckets[0].TotalCount.ShouldBe(2);
        buckets[0].ErrorCount.ShouldBe(1);
        buckets[0].WarningCount.ShouldBe(1);
        buckets[0].PendingCount.ShouldBe(0);
        buckets[0].LeasedCount.ShouldBe(0);
        buckets[0].DeadLetterCount.ShouldBe(0);
        buckets[1].TotalCount.ShouldBe(1);
        buckets[1].ErrorCount.ShouldBe(1);
        buckets[1].WarningCount.ShouldBe(0);
        buckets[1].PendingCount.ShouldBe(0);
        buckets[1].LeasedCount.ShouldBe(0);
        buckets[1].DeadLetterCount.ShouldBe(1);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task RecordAsync_stores_invoke_activity()
    {
        var occurredAt = new DateTimeOffset(2025, 1, 2, 12, 0, 0, TimeSpan.Zero);
        var record = new WebhookActivityRecord(
            Guid.NewGuid().ToString("N"),
            "hook-invoke",
            "samples:invoke",
            TenantId,
            EnvironmentTag,
            occurredAt,
            WebhookActivityStatus.Success,
            WebhookActivitySources.Invoke,
            Reason: null,
            Payload: "{\"ok\":true}",
            Metadata: new Dictionary<string, string> { ["note"] = "manual" });

        await _recorder!.RecordAsync(record, CancellationToken.None);

        var entries = await _store!.ListAsync(
            new PartitionScope(TenantId, EnvironmentTag),
            new WebhookActivityQuery
            {
                FromUtc = occurredAt.AddMinutes(-1),
                ToUtc = occurredAt.AddMinutes(1),
                Limit = 10
            },
            CancellationToken.None);

        entries.ShouldContain(entry =>
            entry.HookKey == "hook-invoke"
            && entry.Kind == WebhookActivityKind.Delivery
            && entry.Source == WebhookActivitySources.Invoke);
    }

    private async Task SeedActivityAsync(
        DateTimeOffset deliveredAt,
        DateTimeOffset failedAt,
        DateTimeOffset deadLetterAt)
    {
        await using var db = await _dbFactory!.CreateDbContextAsync();
        db.WebhookIngressEvents.AddRange(
            new WebhookIngressEventEntity
            {
                EventId = Guid.NewGuid().ToString("N"),
                HookKey = "hook-success",
                JobKey = "samples:success",
                TenantId = TenantId,
                EnvironmentTag = EnvironmentTag,
                Payload = "{}",
                ReceivedAtUtc = deliveredAt.UtcDateTime,
                Status = "Delivered",
                AttemptCount = 2,
                CreatedAtUtc = deliveredAt.UtcDateTime,
                UpdatedAtUtc = deliveredAt.UtcDateTime
            },
            new WebhookIngressEventEntity
            {
                EventId = Guid.NewGuid().ToString("N"),
                HookKey = "hook-fail",
                JobKey = "samples:fail",
                TenantId = TenantId,
                EnvironmentTag = EnvironmentTag,
                Payload = "{}",
                ReceivedAtUtc = failedAt.UtcDateTime,
                Status = "Failed",
                AttemptCount = 1,
                LastError = "boom",
                CreatedAtUtc = failedAt.UtcDateTime,
                UpdatedAtUtc = failedAt.UtcDateTime
            });

        db.WebhookDeadLetters.Add(new WebhookDeadLetterEntity
        {
            HookKey = "hook-dead",
            JobKey = "samples:dead",
            TenantId = TenantId,
            EnvironmentTag = EnvironmentTag,
            Payload = "{}",
            FailureReason = "signature-invalid",
            Attempts = 1,
            CreatedAtUtc = deadLetterAt.UtcDateTime
        });

        await db.SaveChangesAsync();
    }
}
