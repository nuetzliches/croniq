using System;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.SqlServer;
using Croniq.Options;
using Croniq.Sdk;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Auth.SqlServer;

[CroniqJob("croniq", "retention-cleanup")]
public sealed class RetentionCleanupJob : IJob
{
    private readonly IDbContextFactory<SqlServerDbContext> _dbContextFactory;
    private readonly CroniqOptions _coreOptions;
    private readonly CroniqRetentionOptions _retentionOptions;
    private readonly TimeProvider _timeProvider;

    public RetentionCleanupJob(
        IDbContextFactory<SqlServerDbContext> dbContextFactory,
        IOptions<CroniqOptions> coreOptions,
        IOptions<CroniqRetentionOptions> retentionOptions,
        TimeProvider? timeProvider = null)
    {
        _dbContextFactory = dbContextFactory ?? throw new ArgumentNullException(nameof(dbContextFactory));
        _coreOptions = coreOptions?.Value ?? throw new ArgumentNullException(nameof(coreOptions));
        _retentionOptions = retentionOptions?.Value ?? new CroniqRetentionOptions();
        _timeProvider = timeProvider ?? TimeProvider.System;
    }

    public async Task ExecuteAsync(IJobExecutionContext context, CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(context);

        if (!_retentionOptions.Enabled)
        {
            context.Logger.LogInformation("Retention cleanup is disabled.");
            return;
        }

        var tenantId = _coreOptions.TenantId.Trim();
        var environmentTag = _coreOptions.EnvironmentTag;
        var nowUtc = _timeProvider.GetUtcNow().UtcDateTime;

        using var activity = StartActivity(context, tenantId, nowUtc);

        await using var db = await _dbContextFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        var refreshTokensDeleted = await CleanupRefreshTokensAsync(db, tenantId, nowUtc, context.Logger, cancellationToken).ConfigureAwait(false);
        var jobDeadLettersDeleted = await CleanupJobDeadLettersAsync(db, tenantId, environmentTag, nowUtc, context.Logger, cancellationToken).ConfigureAwait(false);
        var webhookDeadLettersDeleted = await CleanupWebhookDeadLettersAsync(db, tenantId, environmentTag, nowUtc, context.Logger, cancellationToken).ConfigureAwait(false);
        var webhookEndpointEventsDeleted = await CleanupWebhookEndpointEventsAsync(db, tenantId, environmentTag, nowUtc, context.Logger, cancellationToken).ConfigureAwait(false);
        var webhookSecretHistoryDeleted = await CleanupWebhookSecretHistoryAsync(db, tenantId, environmentTag, nowUtc, context.Logger, cancellationToken).ConfigureAwait(false);

        context.Logger.LogInformation(
            "Retention cleanup completed for tenant {TenantId}. Deleted refreshTokens={RefreshTokensDeleted}, jobDeadLetters={JobDeadLettersDeleted}, webhookDeadLetters={WebhookDeadLettersDeleted}, webhookEndpointEvents={WebhookEndpointEventsDeleted}, webhookSecretHistory={WebhookSecretHistoryDeleted}.",
            tenantId,
            refreshTokensDeleted,
            jobDeadLettersDeleted,
            webhookDeadLettersDeleted,
            webhookEndpointEventsDeleted,
            webhookSecretHistoryDeleted);
    }

    private async Task<int> CleanupRefreshTokensAsync(
        SqlServerDbContext db,
        string tenantId,
        DateTime nowUtc,
        ILogger logger,
        CancellationToken cancellationToken)
    {
        if (!_retentionOptions.RefreshTokensEnabled || _retentionOptions.RefreshTokensRetentionDays < 0)
        {
            return 0;
        }

        var cutoffUtc = nowUtc.AddDays(-_retentionOptions.RefreshTokensRetentionDays);
        var deleted = await db.RefreshTokens
            .Where(t => t.TenantId == tenantId && t.ExpiresAtUtc < cutoffUtc)
            .ExecuteDeleteAsync(cancellationToken)
            .ConfigureAwait(false);

        logger.LogInformation(
            "Retention cleanup deleted {Deleted} refresh tokens for tenant {TenantId} (expiryOffsetDays={OffsetDays}, cutoff={CutoffUtc:O}).",
            deleted,
            tenantId,
            _retentionOptions.RefreshTokensRetentionDays,
            cutoffUtc);

        return deleted;
    }

    private async Task<int> CleanupJobDeadLettersAsync(
        SqlServerDbContext db,
        string tenantId,
        string environmentTag,
        DateTime nowUtc,
        ILogger logger,
        CancellationToken cancellationToken)
    {
        if (!_retentionOptions.JobDeadLettersEnabled || _retentionOptions.JobDeadLettersExpiryOffsetDays < 0)
        {
            return 0;
        }

        var cutoffUtc = nowUtc.AddDays(-_retentionOptions.JobDeadLettersExpiryOffsetDays);

        // ExecuteDelete cannot reference navigations or joins; dead letters are tenant-scoped via Triggers -> Jobs.
        var deleted = await db.Database.ExecuteSqlInterpolatedAsync(
                $@"DELETE dl
FROM [croniq].[DeadLetters] dl
INNER JOIN [croniq].[Triggers] t ON dl.[TriggerId] = t.[Id]
INNER JOIN [croniq].[Jobs] j ON t.[JobId] = j.[Id]
WHERE j.[TenantId] = {tenantId} AND j.[EnvironmentTag] = {environmentTag} AND dl.[ExpiresAtUtc] < {cutoffUtc};",
                cancellationToken)
            .ConfigureAwait(false);

        logger.LogInformation(
            "Retention cleanup deleted {Deleted} job dead letters for tenant {TenantId} (expiryOffsetDays={OffsetDays}, cutoff={CutoffUtc:O}).",
            deleted,
            tenantId,
            _retentionOptions.JobDeadLettersExpiryOffsetDays,
            cutoffUtc);

        return deleted;
    }

    private async Task<int> CleanupWebhookDeadLettersAsync(
        SqlServerDbContext db,
        string tenantId,
        string environmentTag,
        DateTime nowUtc,
        ILogger logger,
        CancellationToken cancellationToken)
    {
        if (!_retentionOptions.WebhookDeadLettersEnabled || _retentionOptions.WebhookDeadLettersExpiryOffsetDays < 0)
        {
            return 0;
        }

        var cutoffUtc = nowUtc.AddDays(-_retentionOptions.WebhookDeadLettersExpiryOffsetDays);
        var deleted = await db.WebhookDeadLetters
            .Where(d => d.TenantId == tenantId
                        && d.EnvironmentTag == environmentTag
                        && d.ExpiresAtUtc != null
                        && d.ExpiresAtUtc < cutoffUtc)
            .ExecuteDeleteAsync(cancellationToken)
            .ConfigureAwait(false);

        logger.LogInformation(
            "Retention cleanup deleted {Deleted} webhook dead letters for tenant {TenantId} (expiryOffsetDays={OffsetDays}, cutoff={CutoffUtc:O}).",
            deleted,
            tenantId,
            _retentionOptions.WebhookDeadLettersExpiryOffsetDays,
            cutoffUtc);

        return deleted;
    }

    private async Task<int> CleanupWebhookEndpointEventsAsync(
        SqlServerDbContext db,
        string tenantId,
        string environmentTag,
        DateTime nowUtc,
        ILogger logger,
        CancellationToken cancellationToken)
    {
        if (!_retentionOptions.WebhookEndpointEventsEnabled || _retentionOptions.WebhookEndpointEventsRetentionDays < 0)
        {
            return 0;
        }

        var cutoffUtc = nowUtc.AddDays(-_retentionOptions.WebhookEndpointEventsRetentionDays);
        var deleted = await db.WebhookEndpointEvents
            .Where(e => e.TenantId == tenantId
                        && e.EnvironmentTag == environmentTag
                        && e.OccurredAtUtc < cutoffUtc)
            .ExecuteDeleteAsync(cancellationToken)
            .ConfigureAwait(false);

        logger.LogInformation(
            "Retention cleanup deleted {Deleted} webhook endpoint events for tenant {TenantId} (retentionDays={RetentionDays}, cutoff={CutoffUtc:O}).",
            deleted,
            tenantId,
            _retentionOptions.WebhookEndpointEventsRetentionDays,
            cutoffUtc);

        return deleted;
    }

    private async Task<int> CleanupWebhookSecretHistoryAsync(
        SqlServerDbContext db,
        string tenantId,
        string environmentTag,
        DateTime nowUtc,
        ILogger logger,
        CancellationToken cancellationToken)
    {
        if (!_retentionOptions.WebhookSecretHistoryEnabled || _retentionOptions.WebhookSecretHistoryExpiryOffsetDays < 0)
        {
            return 0;
        }

        var cutoffUtc = nowUtc.AddDays(-_retentionOptions.WebhookSecretHistoryExpiryOffsetDays);
        var deleted = await db.WebhookSecretHistory
            .Where(s => s.TenantId == tenantId
                        && s.EnvironmentTag == environmentTag
                        && s.ExpiresAtUtc != null
                        && s.ExpiresAtUtc < cutoffUtc)
            .ExecuteDeleteAsync(cancellationToken)
            .ConfigureAwait(false);

        logger.LogInformation(
            "Retention cleanup deleted {Deleted} webhook secret history entries for tenant {TenantId} (expiryOffsetDays={OffsetDays}, cutoff={CutoffUtc:O}).",
            deleted,
            tenantId,
            _retentionOptions.WebhookSecretHistoryExpiryOffsetDays,
            cutoffUtc);

        return deleted;
    }

    private static Activity? StartActivity(IJobExecutionContext context, string tenantId, DateTime nowUtc)
    {
        var activity = context.ActivitySource.StartActivity("croniq.retention.cleanup");
        activity?.SetTag("croniq.tenant_id", tenantId);
        activity?.SetTag("croniq.retention.now_utc", nowUtc.ToString("O"));
        activity?.SetTag("croniq.job_key", context.JobKey);
        activity?.SetTag("croniq.execution_id", context.ExecutionId);
        return activity;
    }
}