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
using Shouldly;
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

        id.ShouldBeGreaterThan(0);

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var entity = await context.WebhookDeadLetters.SingleAsync(x => x.Id == id);
        entity.HookKey.ShouldBe("tenant-deadletters-dev-alpha");
        entity.HeadersJson.ShouldNotBeNull();
        entity.HeadersJson!.ShouldContain("X-Test");
        entity.MetadataJson.ShouldNotBeNull();
        entity.MetadataJson!.ShouldContain("payload");
        entity.Attempts.ShouldBe(0);
        entity.StatusCode.ShouldBe(401);
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

        entries.Count().ShouldBe(2);
        entries.Select(x => x.HookKey)
            .OrderBy(x => x, StringComparer.Ordinal)
            .ToArray()
            .ShouldBe(new[] { "hook-dev-1", "hook-dev-2" }.OrderBy(x => x, StringComparer.Ordinal).ToArray());
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
        entry.ShouldNotBeNull();
        entry!.FailureReason.ShouldBe("execution-error");
        entry.Attempts.ShouldBe(1);
        entry.StatusCode.ShouldBe(500);
        entry.ErrorDetails.ShouldBe("job failed");
        entry.NextAttemptAtUtc.ShouldNotBeNull();
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ResolveAsync_Removes_entry()
    {
        var scope = new PartitionScope("tenant-resolve", "dev");
        var id = await SeedAsync(scope, "hook-dev-resolve");

        await _store!.ResolveAsync(id, scope, CancellationToken.None);

        var entries = await _store.ListAsync(scope, CancellationToken.None);
        entries.ShouldBeEmpty();
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
