using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Auth.SqlServer;
using Croniq.Data.SqlServer;
using Croniq.Data.SqlServer.Entities;
using Croniq.Options;
using Croniq.Persistence.SqlServer.Tests.Collections;
using Croniq.Sdk;
using Croniq.TestKit.SqlServer;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging.Abstractions;
using Microsoft.Extensions.Options;
using Shouldly;
using Xunit;

namespace Croniq.Persistence.SqlServer.Tests;

[Collection(SqlServerContractTestCollection.Name)]
public sealed class RetentionCleanupJobTests
{
    private readonly SqlServerContainerFixture _fixture;

    public RetentionCleanupJobTests(SqlServerContainerFixture fixture)
    {
        _fixture = fixture;
    }

    [Fact]
    public async Task ExecuteAsync_deletes_expired_refresh_tokens_beyond_cutoff_for_current_tenant_only()
    {
        var tenantId = "tenant-a";
        var otherTenantId = "tenant-b";
        var now = new DateTimeOffset(2025, 12, 25, 12, 0, 0, TimeSpan.Zero);
        var retentionDays = 7;
        var cutoff = now.UtcDateTime.AddDays(-retentionDays);

        await using var provider = SqlServerTestServiceProviderFactory.Create(_fixture.ConnectionString);
        var dbFactory = provider.GetRequiredService<IDbContextFactory<SqlServerDbContext>>();

        await SeedTenantAsync(dbFactory, tenantId, cancellationToken: default);
        await SeedTenantAsync(dbFactory, otherTenantId, cancellationToken: default);

        await SeedRefreshTokenAsync(dbFactory,
            tokenId: "rt_old_a",
            tenantId: tenantId,
            userId: "usr_a",
            tokenHash: "hash_old_a",
            expiresAtUtc: cutoff.AddDays(-1),
            createdAtUtc: cutoff.AddDays(-10),
            cancellationToken: default);

        await SeedRefreshTokenAsync(dbFactory,
            tokenId: "rt_new_a",
            tenantId: tenantId,
            userId: "usr_a",
            tokenHash: "hash_new_a",
            expiresAtUtc: cutoff.AddDays(+1),
            createdAtUtc: cutoff.AddDays(-1),
            cancellationToken: default);

        await SeedRefreshTokenAsync(dbFactory,
            tokenId: "rt_old_b",
            tenantId: otherTenantId,
            userId: "usr_b",
            tokenHash: "hash_old_b",
            expiresAtUtc: cutoff.AddDays(-2),
            createdAtUtc: cutoff.AddDays(-20),
            cancellationToken: default);

        var job = new RetentionCleanupJob(
            dbFactory,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = tenantId, EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqRetentionOptions
            {
                Enabled = true,
                RefreshTokensEnabled = true,
                RefreshTokensRetentionDays = retentionDays
            }),
            new FixedTimeProvider(now));

        await job.ExecuteAsync(new TestExecutionContext("croniq:retention-cleanup"), CancellationToken.None);

        await using var db = await dbFactory.CreateDbContextAsync();

        (await db.RefreshTokens.CountAsync(t => t.TenantId == tenantId && t.TokenId == "rt_old_a")).ShouldBe(0);
        (await db.RefreshTokens.CountAsync(t => t.TenantId == tenantId && t.TokenId == "rt_new_a")).ShouldBe(1);
        (await db.RefreshTokens.CountAsync(t => t.TenantId == otherTenantId && t.TokenId == "rt_old_b")).ShouldBe(1);

        // Ensure we do not touch auth.Users as part of the cleanup.
        (await db.PasswordUsers.CountAsync(t => t.TenantId == tenantId)).ShouldBe(1);
        (await db.PasswordUsers.CountAsync(t => t.TenantId == otherTenantId)).ShouldBe(1);
    }

    [Fact]
    public async Task ExecuteAsync_noops_when_disabled()
    {
        var tenantId = "tenant-disabled";
        var now = new DateTimeOffset(2025, 12, 25, 12, 0, 0, TimeSpan.Zero);

        await using var provider = SqlServerTestServiceProviderFactory.Create(_fixture.ConnectionString);
        var dbFactory = provider.GetRequiredService<IDbContextFactory<SqlServerDbContext>>();
        await SeedTenantAsync(dbFactory, tenantId, cancellationToken: default);

        await SeedRefreshTokenAsync(dbFactory,
            tokenId: "rt_disabled",
            tenantId: tenantId,
            userId: "usr",
            tokenHash: "hash",
            expiresAtUtc: now.UtcDateTime.AddDays(-90),
            createdAtUtc: now.UtcDateTime.AddDays(-90),
            cancellationToken: default);

        var job = new RetentionCleanupJob(
            dbFactory,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = tenantId, EnvironmentTag = "dev", InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqRetentionOptions { Enabled = false }),
            new FixedTimeProvider(now));

        await job.ExecuteAsync(new TestExecutionContext("croniq:retention-cleanup"), CancellationToken.None);

        await using var db = await dbFactory.CreateDbContextAsync();
        (await db.RefreshTokens.CountAsync(t => t.TenantId == tenantId && t.TokenId == "rt_disabled")).ShouldBe(1);
    }

    [Fact]
    public async Task ExecuteAsync_deletes_expired_job_dead_letters_beyond_cutoff_for_current_tenant_and_env_only()
    {
        var tenantId = "tenant-dl-a";
        var otherTenantId = "tenant-dl-b";
        var env = "dev";
        var otherEnv = "prod";

        var now = new DateTimeOffset(2025, 12, 25, 12, 0, 0, TimeSpan.Zero);
        var offsetDays = 2;
        var cutoff = now.UtcDateTime.AddDays(-offsetDays);

        await using var provider = SqlServerTestServiceProviderFactory.Create(_fixture.ConnectionString);
        var dbFactory = provider.GetRequiredService<IDbContextFactory<SqlServerDbContext>>();

        await SeedTenantAsync(dbFactory, tenantId, cancellationToken: default);
        await SeedTenantAsync(dbFactory, otherTenantId, cancellationToken: default);

        var triggerADev = await SeedJobAndTriggerAsync(dbFactory, tenantId, env, jobKey: "croniq/tests/job-dl", triggerKey: "croniq.tests.trg.dl.a.dev", cancellationToken: default);
        var triggerAProd = await SeedJobAndTriggerAsync(dbFactory, tenantId, otherEnv, jobKey: "croniq/tests/job-dl", triggerKey: "croniq.tests.trg.dl.a.prod", cancellationToken: default);
        var triggerBOther = await SeedJobAndTriggerAsync(dbFactory, otherTenantId, env, jobKey: "croniq/tests/job-dl", triggerKey: "croniq.tests.trg.dl.b.dev", cancellationToken: default);

        await SeedDeadLetterAsync(dbFactory, triggerADev, idSuffix: "old_a_dev", expiresAtUtc: cutoff.AddDays(-1), cancellationToken: default);
        await SeedDeadLetterAsync(dbFactory, triggerADev, idSuffix: "new_a_dev", expiresAtUtc: cutoff.AddDays(+1), cancellationToken: default);
        await SeedDeadLetterAsync(dbFactory, triggerAProd, idSuffix: "old_a_prod", expiresAtUtc: cutoff.AddDays(-1), cancellationToken: default);
        await SeedDeadLetterAsync(dbFactory, triggerBOther, idSuffix: "old_b_dev", expiresAtUtc: cutoff.AddDays(-1), cancellationToken: default);

        var job = new RetentionCleanupJob(
            dbFactory,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = tenantId, EnvironmentTag = env, InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqRetentionOptions
            {
                Enabled = true,
                RefreshTokensEnabled = false,
                JobDeadLettersEnabled = true,
                JobDeadLettersExpiryOffsetDays = offsetDays
            }),
            new FixedTimeProvider(now));

        await job.ExecuteAsync(new TestExecutionContext("croniq:retention-cleanup"), CancellationToken.None);

        await using var db = await dbFactory.CreateDbContextAsync();
        (await db.DeadLetters.CountAsync()).ShouldBe(3);
    }

    [Fact]
    public async Task ExecuteAsync_deletes_expired_webhook_dead_letters_only_when_expires_is_set_and_scoped()
    {
        var tenantId = "tenant-whdl-a";
        var otherTenantId = "tenant-whdl-b";
        var env = "dev";

        var now = new DateTimeOffset(2025, 12, 25, 12, 0, 0, TimeSpan.Zero);

        await using var provider = SqlServerTestServiceProviderFactory.Create(_fixture.ConnectionString);
        var dbFactory = provider.GetRequiredService<IDbContextFactory<SqlServerDbContext>>();

        await SeedTenantAsync(dbFactory, tenantId, cancellationToken: default);
        await SeedTenantAsync(dbFactory, otherTenantId, cancellationToken: default);

        await SeedWebhookDeadLetterAsync(dbFactory, tenantId, env, hookKey: "hook1", idHint: 1, expiresAtUtc: now.UtcDateTime.AddDays(-1), cancellationToken: default);
        await SeedWebhookDeadLetterAsync(dbFactory, tenantId, env, hookKey: "hook1", idHint: 2, expiresAtUtc: now.UtcDateTime.AddDays(+1), cancellationToken: default);
        await SeedWebhookDeadLetterAsync(dbFactory, tenantId, env, hookKey: "hook1", idHint: 3, expiresAtUtc: null, cancellationToken: default);
        await SeedWebhookDeadLetterAsync(dbFactory, otherTenantId, env, hookKey: "hook1", idHint: 4, expiresAtUtc: now.UtcDateTime.AddDays(-5), cancellationToken: default);
        await SeedWebhookDeadLetterAsync(dbFactory, tenantId, "prod", hookKey: "hook1", idHint: 5, expiresAtUtc: now.UtcDateTime.AddDays(-5), cancellationToken: default);

        var job = new RetentionCleanupJob(
            dbFactory,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = tenantId, EnvironmentTag = env, InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqRetentionOptions
            {
                Enabled = true,
                RefreshTokensEnabled = false,
                WebhookDeadLettersEnabled = true,
                WebhookDeadLettersExpiryOffsetDays = 0
            }),
            new FixedTimeProvider(now));

        await job.ExecuteAsync(new TestExecutionContext("croniq:retention-cleanup"), CancellationToken.None);

        await using var db = await dbFactory.CreateDbContextAsync();
        (await db.WebhookDeadLetters.CountAsync(d => d.TenantId == tenantId && d.EnvironmentTag == env)).ShouldBe(2);
        (await db.WebhookDeadLetters.CountAsync(d => d.TenantId == otherTenantId)).ShouldBe(1);
        (await db.WebhookDeadLetters.CountAsync(d => d.TenantId == tenantId && d.EnvironmentTag == "prod")).ShouldBe(1);
    }

    [Fact]
    public async Task ExecuteAsync_deletes_webhook_endpoint_events_older_than_retention_days_and_scoped()
    {
        var tenantId = "tenant-whev-a";
        var env = "dev";
        var otherEnv = "prod";

        var now = new DateTimeOffset(2025, 12, 25, 12, 0, 0, TimeSpan.Zero);
        var retentionDays = 7;
        var cutoff = now.UtcDateTime.AddDays(-retentionDays);

        await using var provider = SqlServerTestServiceProviderFactory.Create(_fixture.ConnectionString);
        var dbFactory = provider.GetRequiredService<IDbContextFactory<SqlServerDbContext>>();

        await SeedTenantAsync(dbFactory, tenantId, cancellationToken: default);
        await SeedWebhookEndpointAsync(dbFactory, tenantId, env, hookKey: "hook_evt", cancellationToken: default);
        await SeedWebhookEndpointAsync(dbFactory, tenantId, otherEnv, hookKey: "hook_evt", cancellationToken: default);

        await SeedWebhookEndpointEventAsync(dbFactory, tenantId, env, hookKey: "hook_evt", occurredAtUtc: cutoff.AddDays(-1), cancellationToken: default);
        await SeedWebhookEndpointEventAsync(dbFactory, tenantId, env, hookKey: "hook_evt", occurredAtUtc: cutoff.AddDays(+1), cancellationToken: default);
        await SeedWebhookEndpointEventAsync(dbFactory, tenantId, otherEnv, hookKey: "hook_evt", occurredAtUtc: cutoff.AddDays(-1), cancellationToken: default);

        var job = new RetentionCleanupJob(
            dbFactory,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = tenantId, EnvironmentTag = env, InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqRetentionOptions
            {
                Enabled = true,
                RefreshTokensEnabled = false,
                WebhookEndpointEventsEnabled = true,
                WebhookEndpointEventsRetentionDays = retentionDays
            }),
            new FixedTimeProvider(now));

        await job.ExecuteAsync(new TestExecutionContext("croniq:retention-cleanup"), CancellationToken.None);

        await using var db = await dbFactory.CreateDbContextAsync();
        (await db.WebhookEndpointEvents.CountAsync(e => e.TenantId == tenantId && e.EnvironmentTag == env)).ShouldBe(1);
        (await db.WebhookEndpointEvents.CountAsync(e => e.TenantId == tenantId && e.EnvironmentTag == otherEnv)).ShouldBe(1);
    }

    [Fact]
    public async Task ExecuteAsync_deletes_webhook_secret_history_only_when_expires_is_set_and_scoped()
    {
        var tenantId = "tenant-whsh-a";
        var env = "dev";
        var otherEnv = "prod";

        var now = new DateTimeOffset(2025, 12, 25, 12, 0, 0, TimeSpan.Zero);

        await using var provider = SqlServerTestServiceProviderFactory.Create(_fixture.ConnectionString);
        var dbFactory = provider.GetRequiredService<IDbContextFactory<SqlServerDbContext>>();

        await SeedTenantAsync(dbFactory, tenantId, cancellationToken: default);
        await SeedWebhookEndpointAsync(dbFactory, tenantId, env, hookKey: "hook_sh", cancellationToken: default);
        await SeedWebhookEndpointAsync(dbFactory, tenantId, otherEnv, hookKey: "hook_sh", cancellationToken: default);

        await SeedWebhookSecretHistoryAsync(dbFactory, tenantId, env, hookKey: "hook_sh", idHint: 1, expiresAtUtc: now.UtcDateTime.AddDays(-1), cancellationToken: default);
        await SeedWebhookSecretHistoryAsync(dbFactory, tenantId, env, hookKey: "hook_sh", idHint: 2, expiresAtUtc: now.UtcDateTime.AddDays(+1), cancellationToken: default);
        await SeedWebhookSecretHistoryAsync(dbFactory, tenantId, env, hookKey: "hook_sh", idHint: 3, expiresAtUtc: null, cancellationToken: default);
        await SeedWebhookSecretHistoryAsync(dbFactory, tenantId, otherEnv, hookKey: "hook_sh", idHint: 4, expiresAtUtc: now.UtcDateTime.AddDays(-5), cancellationToken: default);

        var job = new RetentionCleanupJob(
            dbFactory,
            Microsoft.Extensions.Options.Options.Create(new CroniqOptions { TenantId = tenantId, EnvironmentTag = env, InstanceId = "i1" }),
            Microsoft.Extensions.Options.Options.Create(new CroniqRetentionOptions
            {
                Enabled = true,
                RefreshTokensEnabled = false,
                WebhookSecretHistoryEnabled = true,
                WebhookSecretHistoryExpiryOffsetDays = 0
            }),
            new FixedTimeProvider(now));

        await job.ExecuteAsync(new TestExecutionContext("croniq:retention-cleanup"), CancellationToken.None);

        await using var db = await dbFactory.CreateDbContextAsync();
        (await db.WebhookSecretHistory.CountAsync(s => s.TenantId == tenantId && s.EnvironmentTag == env)).ShouldBe(2);
        (await db.WebhookSecretHistory.CountAsync(s => s.TenantId == tenantId && s.EnvironmentTag == otherEnv)).ShouldBe(1);
    }

    private static async Task SeedTenantAsync(IDbContextFactory<SqlServerDbContext> factory, string tenantId, CancellationToken cancellationToken)
    {
        await using var db = await factory.CreateDbContextAsync(cancellationToken);
        db.Tenants.Add(new TenantEntity
        {
            TenantId = tenantId,
            Name = tenantId,
            IsActive = true,
            CreatedAtUtc = DateTime.UtcNow,
            UpdatedAtUtc = DateTime.UtcNow
        });

        db.PasswordUsers.Add(new PasswordUserEntity
        {
            UserId = $"u_{tenantId}",
            TenantId = tenantId,
            Username = "admin",
            UsernameNormalized = "ADMIN",
            PasswordHash = "hash",
            ScopesJson = "[]",
            IsActive = true,
            FailedLoginCount = 0,
            LockoutEndUtc = null,
            PasswordChangeRequired = false,
            CreatedAtUtc = DateTime.UtcNow,
            UpdatedAtUtc = DateTime.UtcNow
        });

        await db.SaveChangesAsync(cancellationToken);
    }

    private static async Task SeedRefreshTokenAsync(
        IDbContextFactory<SqlServerDbContext> factory,
        string tokenId,
        string tenantId,
        string userId,
        string tokenHash,
        DateTime expiresAtUtc,
        DateTime createdAtUtc,
        CancellationToken cancellationToken)
    {
        await using var db = await factory.CreateDbContextAsync(cancellationToken);
        db.RefreshTokens.Add(new RefreshTokenEntity
        {
            TokenId = tokenId,
            TenantId = tenantId,
            UserId = userId,
            TokenHash = tokenHash,
            ExpiresAtUtc = expiresAtUtc,
            RevokedAtUtc = null,
            ReplacedByTokenId = null,
            CreatedAtUtc = createdAtUtc
        });
        await db.SaveChangesAsync(cancellationToken);
    }

    private static async Task<long> SeedJobAndTriggerAsync(
        IDbContextFactory<SqlServerDbContext> factory,
        string tenantId,
        string environmentTag,
        string jobKey,
        string triggerKey,
        CancellationToken cancellationToken)
    {
        await using var db = await factory.CreateDbContextAsync(cancellationToken);

        var job = new JobEntity
        {
            JobKey = jobKey,
            TenantId = tenantId,
            EnvironmentTag = environmentTag,
            NamespaceSegment = "tests",
            Name = "job",
            Variant = null,
            Description = null,
            MetadataJson = null,
            CreatedAtUtc = DateTime.UtcNow,
            UpdatedAtUtc = DateTime.UtcNow
        };

        db.Jobs.Add(job);
        await db.SaveChangesAsync(cancellationToken);

        var trigger = new TriggerEntity
        {
            TriggerKey = triggerKey,
            JobKey = job.JobKey,
            JobId = job.Id,
            CronExpression = "0 0 3 ? * * *",
            TimeZoneId = "UTC",
            Enabled = true,
            IsDeleted = false,
            CreatedAtUtc = DateTime.UtcNow,
            UpdatedAtUtc = DateTime.UtcNow,
            MetadataJson = null,
            NextFireAtUtc = null,
            LastFiredAtUtc = null,
            LastCompletedAtUtc = null,
            LastResult = null,
            LeaseId = null,
            LeaseInstanceId = null,
            LeaseExpiresAtUtc = null,
            StartAtUtc = null,
            EndAtUtc = null
        };

        db.Triggers.Add(trigger);
        await db.SaveChangesAsync(cancellationToken);

        return trigger.Id;
    }

    private static async Task SeedDeadLetterAsync(
        IDbContextFactory<SqlServerDbContext> factory,
        long triggerId,
        string idSuffix,
        DateTime expiresAtUtc,
        CancellationToken cancellationToken)
    {
        await using var db = await factory.CreateDbContextAsync(cancellationToken);
        db.DeadLetters.Add(new DeadLetterEntity
        {
            TriggerId = triggerId,
            FireAtUtc = DateTime.UtcNow,
            Reason = $"reason_{idSuffix}",
            Payload = "{}",
            MetadataJson = null,
            CreatedAtUtc = DateTime.UtcNow,
            ExpiresAtUtc = expiresAtUtc
        });
        await db.SaveChangesAsync(cancellationToken);
    }

    private static async Task SeedWebhookDeadLetterAsync(
        IDbContextFactory<SqlServerDbContext> factory,
        string tenantId,
        string environmentTag,
        string hookKey,
        int idHint,
        DateTime? expiresAtUtc,
        CancellationToken cancellationToken)
    {
        await using var db = await factory.CreateDbContextAsync(cancellationToken);
        db.WebhookDeadLetters.Add(new WebhookDeadLetterEntity
        {
            HookKey = hookKey,
            JobKey = "croniq/tests/webhook-job",
            TenantId = tenantId,
            EnvironmentTag = environmentTag,
            Payload = "{}",
            HeadersJson = null,
            MetadataJson = null,
            FailureReason = $"fail_{idHint}",
            ErrorDetails = null,
            StatusCode = null,
            Attempts = 0,
            CreatedAtUtc = DateTime.UtcNow,
            LastAttemptAtUtc = null,
            NextAttemptAtUtc = null,
            ExpiresAtUtc = expiresAtUtc
        });
        await db.SaveChangesAsync(cancellationToken);
    }

    private static async Task SeedWebhookEndpointAsync(
        IDbContextFactory<SqlServerDbContext> factory,
        string tenantId,
        string environmentTag,
        string hookKey,
        CancellationToken cancellationToken)
    {
        await using var db = await factory.CreateDbContextAsync(cancellationToken);
        db.WebhookEndpoints.Add(new WebhookEndpointEntity
        {
            HookKey = hookKey,
            TenantId = tenantId,
            EnvironmentTag = environmentTag,
            JobKey = "croniq/tests/webhook-job",
            Secret = "secret",
            SecretHash = "hash",
            SignatureVersion = 1,
            RequestsPerMinute = 0,
            Enabled = true,
            RequireSignature = true,
            MetadataJson = null,
            IsDeleted = false,
            CreatedAtUtc = DateTime.UtcNow,
            UpdatedAtUtc = DateTime.UtcNow
        });
        await db.SaveChangesAsync(cancellationToken);
    }

    private static async Task SeedWebhookEndpointEventAsync(
        IDbContextFactory<SqlServerDbContext> factory,
        string tenantId,
        string environmentTag,
        string hookKey,
        DateTime occurredAtUtc,
        CancellationToken cancellationToken)
    {
        await using var db = await factory.CreateDbContextAsync(cancellationToken);
        db.WebhookEndpointEvents.Add(new WebhookEndpointEventEntity
        {
            HookKey = hookKey,
            TenantId = tenantId,
            EnvironmentTag = environmentTag,
            EventType = "Updated",
            OccurredAtUtc = occurredAtUtc,
            Actor = null,
            CorrelationId = null
        });
        await db.SaveChangesAsync(cancellationToken);
    }

    private static async Task SeedWebhookSecretHistoryAsync(
        IDbContextFactory<SqlServerDbContext> factory,
        string tenantId,
        string environmentTag,
        string hookKey,
        int idHint,
        DateTime? expiresAtUtc,
        CancellationToken cancellationToken)
    {
        await using var db = await factory.CreateDbContextAsync(cancellationToken);
        db.WebhookSecretHistory.Add(new WebhookSecretHistoryEntity
        {
            HookKey = hookKey,
            TenantId = tenantId,
            EnvironmentTag = environmentTag,
            Secret = $"secret_{idHint}",
            SecretHash = $"hash_{idHint}",
            ActivatedAtUtc = DateTime.UtcNow,
            ExpiresAtUtc = expiresAtUtc,
            RotatedBy = null,
            Notes = null
        });
        await db.SaveChangesAsync(cancellationToken);
    }

    private sealed class TestExecutionContext : IJobExecutionContext
    {
        public TestExecutionContext(string jobKey)
        {
            JobKey = jobKey;
        }

        public string ExecutionId { get; } = "exec-retention";
        public string JobKey { get; }
        public IReadOnlyDictionary<string, string> Metadata { get; } = new Dictionary<string, string>();
        public Microsoft.Extensions.Logging.ILogger Logger { get; } = NullLogger.Instance;
        public ActivitySource ActivitySource { get; } = new("Croniq.Persistence.SqlServer.Tests");
    }

    private sealed class FixedTimeProvider : TimeProvider
    {
        private readonly DateTimeOffset _now;

        public FixedTimeProvider(DateTimeOffset now)
        {
            _now = now;
        }

        public override DateTimeOffset GetUtcNow() => _now;
    }
}
