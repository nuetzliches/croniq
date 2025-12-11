using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.SqlServer;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer;
using Croniq.Persistence.SqlServer.Tests.Collections;
using Croniq.TestKit.SqlServer;
using Croniq.TestKit.Testing;
using FluentAssertions;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests;

[Collection(SqlServerContractTestCollection.Name)]
[Trait(TestTraits.Component, TestTraits.Components.SqlPersistenceDeadLetters)]
public sealed class SqlServerWebhookDeadLetterStoreTests : IAsyncLifetime
{
    private readonly SqlServerContainerFixture _sql;
    private ServiceProvider? _provider;
    private IWebhookDeadLetterStore? _store;
    private IDbContextFactory<SqlServerDbContext>? _dbFactory;

    public SqlServerWebhookDeadLetterStoreTests(SqlServerContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        _provider = BuildServiceProvider(_sql.ConnectionString);
        _store = _provider.GetRequiredService<IWebhookDeadLetterStore>();
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
    public async Task CreateAsync_Persists_payload_headers_and_metadata()
    {
        var scope = new PartitionScope("tenant-deadletters", "dev");
        var metadata = new Dictionary<string, string> { ["webhook:payload"] = "{\"id\":1}" };
        var headers = new Dictionary<string, string> { ["X-Test"] = "value" };

        var id = await _store!.CreateAsync(
            new WebhookDeadLetterCreate(
                HookKey: "tenant-deadletters-dev-alpha",
                JobKey: "tenant-deadletters:dev:ops:alpha",
                TenantId: scope.TenantId,
                EnvironmentTag: scope.EnvironmentTag,
                Payload: "{\"id\":1}",
                Headers: headers,
                Metadata: metadata,
                FailureReason: "signature-invalid",
                StatusCode: 401,
                ErrorDetails: "signature mismatch",
                ExpiresAtUtc: DateTimeOffset.UtcNow.AddDays(7)),
            CancellationToken.None);

        id.Should().BeGreaterThan(0);

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var entity = await context.WebhookDeadLetters.SingleAsync(x => x.Id == id);
        entity.HookKey.Should().Be("tenant-deadletters-dev-alpha");
        entity.HeadersJson.Should().Contain("X-Test");
        entity.MetadataJson.Should().Contain("payload");
        entity.Attempts.Should().Be(0);
        entity.StatusCode.Should().Be(401);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ListAsync_Filters_by_scope()
    {
        var scope = new PartitionScope("tenant-deadletters", "dev");
        var otherScope = new PartitionScope("tenant-deadletters", "qa");
        await SeedAsync(scope, "hook-dev-1");
        await SeedAsync(scope, "hook-dev-2");
        await SeedAsync(otherScope, "hook-qa-1");

        var entries = await _store!.ListAsync(scope, CancellationToken.None);

        entries.Should().HaveCount(2);
        entries.Select(x => x.HookKey).Should().Contain(new[] { "hook-dev-1", "hook-dev-2" });
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task RecordFailureAsync_Increments_attempts_and_updates_reason()
    {
        var scope = new PartitionScope("tenant-replay", "dev");
        var id = await SeedAsync(scope, "hook-dev-1");

        await _store!.RecordFailureAsync(
            id,
            scope,
            new WebhookDeadLetterFailure("execution-error", 500, "job failed", DateTimeOffset.UtcNow.AddMinutes(5)),
            CancellationToken.None);

        var entry = await _store.FindAsync(id, scope, CancellationToken.None);
        entry.Should().NotBeNull();
        entry!.FailureReason.Should().Be("execution-error");
        entry.Attempts.Should().Be(1);
        entry.StatusCode.Should().Be(500);
        entry.ErrorDetails.Should().Be("job failed");
        entry.NextAttemptAtUtc.Should().NotBeNull();
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ResolveAsync_Removes_entry()
    {
        var scope = new PartitionScope("tenant-resolve", "dev");
        var id = await SeedAsync(scope, "hook-dev-resolve");

        await _store!.ResolveAsync(id, scope, CancellationToken.None);

        var entries = await _store.ListAsync(scope, CancellationToken.None);
        entries.Should().BeEmpty();
    }

    private async Task<long> SeedAsync(PartitionScope scope, string hookKey)
    {
        return await _store!.CreateAsync(
            new WebhookDeadLetterCreate(
                HookKey: hookKey,
                JobKey: $"{scope.TenantId}:{scope.EnvironmentTag}:ops:{hookKey}",
                TenantId: scope.TenantId,
                EnvironmentTag: scope.EnvironmentTag,
                Payload: "{}",
                Headers: null,
                Metadata: null,
                FailureReason: "signature-missing",
                StatusCode: 401,
                ErrorDetails: "missing header",
                ExpiresAtUtc: DateTimeOffset.UtcNow.AddDays(1)),
            CancellationToken.None);
    }

    private static ServiceProvider BuildServiceProvider(string connectionString)
    {
        var services = new ServiceCollection();
        services.AddLogging(builder => builder.AddSimpleConsole());
        services.AddCroniqSqlServerPersistence(
            sql =>
            {
                sql.ConnectionString = connectionString;
                sql.EnableDetailedErrors = true;
                sql.EnableSensitiveDataLogging = true;
            });

        return services.BuildServiceProvider();
    }
}
