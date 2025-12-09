using System;
using System.Collections.Generic;
using System.Linq;
using System.Security.Cryptography;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
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
public sealed class SqlServerWebhookPersistenceProviderTests : IAsyncLifetime
{
    private readonly SqlServerContainerFixture _sql;
    private ServiceProvider? _provider;
    private IWebhookPersistenceProvider? _persistence;
    private IDbContextFactory<SqlServerDbContext>? _dbFactory;

    public SqlServerWebhookPersistenceProviderTests(SqlServerContainerFixture sql)
    {
        _sql = sql;
    }

    public async Task InitializeAsync()
    {
        await _sql.ResetDatabaseAsync();
        _provider = BuildServiceProvider(_sql.ConnectionString);
        _persistence = _provider.GetRequiredService<IWebhookPersistenceProvider>();
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
    public async Task UpsertAsync_CreatesAndUpdatesWebhook()
    {
        var jobKey = JobKey.Create("tenant-hooks", "dev", "webhooks", "dispatch");
        var hookKey = "tenant-hooks-dev-dispatch";
        var metadata = new Dictionary<string, string> { ["source"] = "billing" };

        await _persistence!.UpsertAsync(
            new WebhookEndpointUpsert(
                hookKey,
                jobKey.Value,
                jobKey.TenantId,
                jobKey.EnvironmentTag,
                Enabled: true,
                RequireSignature: true,
                RequestsPerMinute: 120,
                Secret: "secret-one",
                SignatureVersion: 2,
                Metadata: metadata),
            CancellationToken.None);

        await using (var context = await _dbFactory!.CreateDbContextAsync())
        {
            var entity = await context.WebhookEndpoints.SingleAsync(x => x.HookKey == hookKey);
            entity.JobKey.Should().Be(jobKey.Value);
            entity.SecretHash.Should().Be(ComputeSecretHash("secret-one"));
            entity.MetadataJson.Should().Contain("billing");
        }

        var updateMetadata = new Dictionary<string, string> { ["source"] = "ops" };
        await _persistence.UpsertAsync(
            new WebhookEndpointUpsert(
                hookKey,
                jobKey.Value,
                jobKey.TenantId,
                jobKey.EnvironmentTag,
                Enabled: false,
                RequireSignature: false,
                RequestsPerMinute: 60,
                Secret: null,
                SignatureVersion: 3,
                Metadata: updateMetadata),
            CancellationToken.None);

        await using (var updatedContext = await _dbFactory.CreateDbContextAsync())
        {
            var updated = await updatedContext.WebhookEndpoints.SingleAsync(x => x.HookKey == hookKey);
            updated.Enabled.Should().BeFalse();
            updated.RequireSignature.Should().BeFalse();
            updated.Secret.Should().Be("secret-one");
            updated.SecretHash.Should().Be(ComputeSecretHash("secret-one"));
        }

        await _persistence.UpsertAsync(
            new WebhookEndpointUpsert(
                hookKey,
                jobKey.Value,
                jobKey.TenantId,
                jobKey.EnvironmentTag,
                Enabled: true,
                RequireSignature: true,
                RequestsPerMinute: 200,
                Secret: "secret-two",
                SignatureVersion: 4,
                Metadata: updateMetadata),
            CancellationToken.None);

        await using var verification = await _dbFactory.CreateDbContextAsync();
        var finalEntity = await verification.WebhookEndpoints.SingleAsync(x => x.HookKey == hookKey);
        finalEntity.Secret.Should().Be("secret-two");
        finalEntity.SecretHash.Should().Be(ComputeSecretHash("secret-two"));
        finalEntity.SignatureVersion.Should().Be(4);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task ListAsync_ReturnsEndpointsWithinScope()
    {
        var tenantScope = new PartitionScope("tenant-scope", "dev");
        var otherScope = new PartitionScope("tenant-scope", "qa");
        var hookA = "tenant-scope-dev-alpha";
        var hookB = "tenant-scope-dev-beta";
        var hookC = "tenant-scope-qa";

        await UpsertEndpointAsync(hookA, tenantScope, JobKey.Create("tenant-scope", "dev", "ops", "alpha"));
        await UpsertEndpointAsync(hookB, tenantScope, JobKey.Create("tenant-scope", "dev", "ops", "beta"));
        await UpsertEndpointAsync(hookC, otherScope, JobKey.Create("tenant-scope", "qa", "ops", "qa"));

        var results = await _persistence!.ListAsync(tenantScope, CancellationToken.None);

        results.Should().HaveCount(2);
        results.Select(x => x.HookKey).Should().BeEquivalentTo(new[] { hookA, hookB });
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task DeleteAsync_RemovesEndpointForScope()
    {
        var scope = new PartitionScope("tenant-delete", "dev");
        var jobKey = JobKey.Create(scope.TenantId, scope.EnvironmentTag, "ops", "cleanup");
        var hookKey = "tenant-delete-dev-hook";

        await UpsertEndpointAsync(hookKey, scope, jobKey);

        await _persistence!.DeleteAsync(hookKey, scope, CancellationToken.None);

        await using (var context = await _dbFactory!.CreateDbContextAsync())
        {
            var exists = await context.WebhookEndpoints.AnyAsync(x => x.HookKey == hookKey);
            exists.Should().BeFalse();
        }

        await UpsertEndpointAsync("tenant-delete-dev-hook-2", scope, jobKey);

        var wrongScope = new PartitionScope("tenant-delete", "qa");
        await FluentActions.Awaiting(() => _persistence.DeleteAsync("tenant-delete-dev-hook-2", wrongScope, CancellationToken.None))
            .Should().ThrowAsync<InvalidOperationException>();
    }

    private async Task UpsertEndpointAsync(string hookKey, PartitionScope scope, JobKey jobKey)
    {
        await _persistence!.UpsertAsync(
            new WebhookEndpointUpsert(
                hookKey,
                jobKey.Value,
                scope.TenantId,
                scope.EnvironmentTag,
                Enabled: true,
                RequireSignature: true,
                RequestsPerMinute: 30,
                Secret: "seed-secret",
                SignatureVersion: 1,
                Metadata: null),
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

    private static string ComputeSecretHash(string secret)
    {
        var bytes = System.Text.Encoding.UTF8.GetBytes(secret);
        var hash = SHA256.HashData(bytes);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }
}

