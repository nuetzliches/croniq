using System;
using System.Collections.Generic;
using System.Linq;
using System.Security.Cryptography;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Data.SqlServer;
using Croniq.Data.SqlServer.Entities;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.SqlServer.Tests.Collections;
using Croniq.TestKit.SqlServer;
using Croniq.TestKit.Testing;
using FluentAssertions;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests;

[Collection(SqlServerContractTestCollection.Name)]
[Trait(TestTraits.Component, TestTraits.Components.SqlPersistenceWebhooks)]
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
        _provider = SqlServerTestServiceProviderFactory.Create(_sql.ConnectionString);
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

        var history = await verification.WebhookSecretHistory
            .Where(x => x.HookKey == hookKey)
            .OrderBy(x => x.ActivatedAtUtc)
            .ToListAsync();
        history.Should().HaveCount(3);
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

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task RotateSecretAsync_AppendsHistoryAndKeepsPreviousActive()
    {
        var scope = new PartitionScope("tenant-rotate", "dev");
        var jobKey = JobKey.Create(scope.TenantId, scope.EnvironmentTag, "ops", "hook");
        var hookKey = "tenant-rotate-dev-hook";
        await UpsertEndpointAsync(hookKey, scope, jobKey);

        var result = await _persistence!.RotateSecretAsync(
            new WebhookSecretRotate(
                hookKey,
                scope.TenantId,
                scope.EnvironmentTag,
                ActivateInSeconds: null,
                GracePeriodSeconds: 300,
                RotatedBy: "tests",
                Notes: "rotate"),
            CancellationToken.None);

        result.HookKey.Should().Be(hookKey);
        result.Secret.Should().NotBeNullOrWhiteSpace();
        result.ExpiresAtUtc.Should().BeNull();

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var history = await context.WebhookSecretHistory
            .Where(x => x.HookKey == hookKey)
            .OrderBy(x => x.ActivatedAtUtc)
            .ToListAsync();

        history.Should().HaveCount(2);
        history.Last().Secret.Should().Be(result.Secret);
        history.First().ExpiresAtUtc.Should().NotBeNull();
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task RotateSecretAsync_WithDelayedActivation_LeavesCurrentSecretActive()
    {
        var scope = new PartitionScope("tenant-rotate-delay", "dev");
        var jobKey = JobKey.Create(scope.TenantId, scope.EnvironmentTag, "ops", "hook");
        var hookKey = "tenant-rotate-delay-dev-hook";
        await UpsertEndpointAsync(hookKey, scope, jobKey);

        var delaySeconds = 600;
        var graceSeconds = 900;
        var rotateResult = await _persistence!.RotateSecretAsync(
            new WebhookSecretRotate(
                hookKey,
                scope.TenantId,
                scope.EnvironmentTag,
                ActivateInSeconds: delaySeconds,
                GracePeriodSeconds: graceSeconds,
                RotatedBy: "tests",
                Notes: "delayed"),
            CancellationToken.None);

        rotateResult.HookKey.Should().Be(hookKey);
        rotateResult.ActivatedAtUtc.Should().BeAfter(DateTime.UtcNow);

        var activeSecrets = await _persistence.GetActiveSecretsAsync(hookKey, CancellationToken.None);
        activeSecrets.Should().HaveCount(1);
        var remainingSecret = activeSecrets.Single();
        remainingSecret.ExpiresAtUtc.Should().NotBeNull();
        remainingSecret.ExpiresAtUtc.Should().BeAfter(rotateResult.ActivatedAtUtc);

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var history = await context.WebhookSecretHistory
            .Where(x => x.HookKey == hookKey)
            .OrderBy(x => x.ActivatedAtUtc)
            .ToListAsync();

        history.Should().HaveCount(2);
        var futureSecret = history.Last();
        futureSecret.ActivatedAtUtc.Should().BeCloseTo(rotateResult.ActivatedAtUtc, TimeSpan.FromSeconds(1));
        futureSecret.ExpiresAtUtc.Should().BeNull();
        var previousSecret = history.First();
        previousSecret.ExpiresAtUtc.Should().NotBeNull();
        previousSecret.ExpiresAtUtc.Should().BeAfter(rotateResult.ActivatedAtUtc);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task RotateSecretAsync_WithActivationDelayBeyondLimit_Throws()
    {
        var scope = new PartitionScope("tenant-rotate-limit", "dev");
        var jobKey = JobKey.Create(scope.TenantId, scope.EnvironmentTag, "ops", "hook");
        var hookKey = "tenant-rotate-limit-dev-hook";
        await UpsertEndpointAsync(hookKey, scope, jobKey);

        var excessiveDelaySeconds = (int)TimeSpan.FromDays(8).TotalSeconds;

        var action = () => _persistence!.RotateSecretAsync(
            new WebhookSecretRotate(
                hookKey,
                scope.TenantId,
                scope.EnvironmentTag,
                ActivateInSeconds: excessiveDelaySeconds,
                GracePeriodSeconds: null,
                RotatedBy: "tests",
                Notes: "limit"),
            CancellationToken.None);

        await action.Should().ThrowAsync<InvalidOperationException>()
            .WithMessage("*ActivateInSeconds*");
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task GetActiveSecretsAsync_WhenOnlyFutureSecretsExist_ReturnsEmpty()
    {
        var scope = new PartitionScope("tenant-future-only", "dev");
        var jobKey = JobKey.Create(scope.TenantId, scope.EnvironmentTag, "ops", "hook");
        var hookKey = "tenant-future-only-dev-hook";
        await UpsertEndpointAsync(hookKey, scope, jobKey);

        await using (var context = await _dbFactory!.CreateDbContextAsync())
        {
            var existing = await context.WebhookSecretHistory
                .Where(x => x.HookKey == hookKey)
                .SingleAsync();

            existing.ExpiresAtUtc = DateTime.UtcNow.AddMinutes(-5);

            context.WebhookSecretHistory.Add(new WebhookSecretHistoryEntity
            {
                HookKey = hookKey,
                TenantId = scope.TenantId,
                EnvironmentTag = scope.EnvironmentTag,
                Secret = "future-secret",
                SecretHash = ComputeSecretHash("future-secret"),
                ActivatedAtUtc = DateTime.UtcNow.AddMinutes(10),
                ExpiresAtUtc = null,
                RotatedBy = "tests"
            });

            await context.SaveChangesAsync();
        }

        var secrets = await _persistence!.GetActiveSecretsAsync(hookKey, CancellationToken.None);
        secrets.Should().BeEmpty();
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task GetActiveSecretsAsync_ReturnsCurrentAndPreviousWithinGrace()
    {
        var scope = new PartitionScope("tenant-active-secrets", "dev");
        var jobKey = JobKey.Create(scope.TenantId, scope.EnvironmentTag, "ops", "hook");
        var hookKey = "tenant-active-dev-hook";
        await UpsertEndpointAsync(hookKey, scope, jobKey);

        await _persistence!.RotateSecretAsync(
            new WebhookSecretRotate(
                hookKey,
                scope.TenantId,
                scope.EnvironmentTag,
                ActivateInSeconds: null,
                GracePeriodSeconds: 120,
                RotatedBy: "tests",
                Notes: null),
            CancellationToken.None);

        var secrets = await _persistence.GetActiveSecretsAsync(hookKey, CancellationToken.None);
        secrets.Should().HaveCount(2);
        secrets.Last().ExpiresAtUtc.Should().BeNull();
        secrets.First().ExpiresAtUtc.Should().NotBeNull();
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task UpsertAsync_WritesChangefeedEvents()
    {
        var scope = new PartitionScope("tenant-changefeed", "dev");
        var jobKey = JobKey.Create(scope.TenantId, scope.EnvironmentTag, "ops", "delta");
        var hookKey = "tenant-changefeed-dev-delta";

        await UpsertEndpointAsync(hookKey, scope, jobKey);
        await _persistence!.UpsertAsync(
            new WebhookEndpointUpsert(
                hookKey,
                jobKey.Value,
                scope.TenantId,
                scope.EnvironmentTag,
                Enabled: false,
                RequireSignature: true,
                RequestsPerMinute: 45,
                Secret: null,
                SignatureVersion: 2,
                Metadata: null),
            CancellationToken.None);

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var events = await context.WebhookEndpointEvents
            .Where(x => x.HookKey == hookKey)
            .OrderBy(x => x.Id)
            .ToListAsync();

        events.Should().HaveCount(2);
        events[0].EventType.Should().Be(WebhookEndpointEventTypes.Created);
        events[1].EventType.Should().Be(WebhookEndpointEventTypes.Updated);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task DeleteAsync_WritesChangefeedEvent()
    {
        var scope = new PartitionScope("tenant-changefeed-delete", "dev");
        var jobKey = JobKey.Create(scope.TenantId, scope.EnvironmentTag, "ops", "omega");
        var hookKey = "tenant-changefeed-delete-dev";

        await UpsertEndpointAsync(hookKey, scope, jobKey);
        await _persistence!.DeleteAsync(hookKey, scope, CancellationToken.None);

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var evt = await context.WebhookEndpointEvents
            .Where(x => x.HookKey == hookKey)
            .OrderBy(x => x.Id)
            .LastAsync();

        evt.EventType.Should().Be(WebhookEndpointEventTypes.Deleted);
    }

    [Fact]
    [Trait(TestCategories.Category, TestCategories.Contract)]
    public async Task IpRuleMutations_RecordActorAndCorrelation()
    {
        var scope = new PartitionScope("tenant-iprules-audit", "dev");
        var jobKey = JobKey.Create(scope.TenantId, scope.EnvironmentTag, "ops", "audit");
        var hookKey = "tenant-iprules-audit-dev";
        await UpsertEndpointAsync(hookKey, scope, jobKey);

        var createdRule = await _persistence!.AddIpRuleAsync(
            new WebhookIpRuleCreate(
                hookKey,
                scope.TenantId,
                scope.EnvironmentTag,
                "198.51.100.0/24",
                "audit",
                "sdk:tests",
                "corr-create"),
            CancellationToken.None);

        await _persistence.DeleteIpRuleAsync(
            createdRule.Id,
            scope,
            "ui:tests",
            "corr-delete",
            CancellationToken.None);

        await using var context = await _dbFactory!.CreateDbContextAsync();
        var auditEvents = await context.WebhookEndpointEvents
            .Where(x => x.HookKey == hookKey && x.CorrelationId != null)
            .OrderBy(x => x.Id)
            .ToListAsync();

        auditEvents.Should().ContainSingle(evt => evt.CorrelationId == "corr-create" && evt.Actor == "sdk:tests");
        auditEvents.Should().ContainSingle(evt => evt.CorrelationId == "corr-delete" && evt.Actor == "ui:tests");
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

    private static string ComputeSecretHash(string secret)
    {
        var bytes = System.Text.Encoding.UTF8.GetBytes(secret);
        var hash = SHA256.HashData(bytes);
        return Convert.ToHexString(hash).ToLowerInvariant();
    }
}

