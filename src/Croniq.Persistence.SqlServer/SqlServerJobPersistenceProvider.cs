using System;
using System.Collections.Generic;
using System.Data;
using System.Linq;
using System.Text.Json;
using Croniq.Core.Jobs;
using Croniq.Core.Scheduling;
using Croniq.Data.SqlServer;
using Croniq.Data.SqlServer.Entities;
using Croniq.Persistence.Abstractions;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Persistence.SqlServer;

/// <summary>
/// EF Core backed implementation of <see cref="IJobPersistenceProvider"/>.
/// </summary>
public sealed class SqlServerJobPersistenceProvider : IJobPersistenceProvider
{
    private readonly IDbContextFactory<SqlServerDbContext> _dbFactory;
    private readonly ILogger<SqlServerJobPersistenceProvider> _logger;
    private readonly SqlServerPersistenceOptions _options;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);

    public SqlServerJobPersistenceProvider(
        IDbContextFactory<SqlServerDbContext> dbFactory,
        IOptions<SqlServerPersistenceOptions> options,
        ILogger<SqlServerJobPersistenceProvider> logger)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        _options = options?.Value ?? new SqlServerPersistenceOptions();
        _options.Normalize();
    }

    public async Task UpsertJobAsync(JobDefinition job, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (job is null) throw new ArgumentNullException(nameof(job));
        if (!JobKey.TryParse(job.JobKey, out var jobKey))
        {
            throw new InvalidOperationException($"JobKey '{job.JobKey}' is invalid.");
        }

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var existing = await db.Jobs.FirstOrDefaultAsync(
            j => j.JobKey == job.JobKey
                 && j.TenantId == scope.TenantId
                 && j.EnvironmentTag == scope.EnvironmentTag,
            cancellationToken).ConfigureAwait(false);
        var now = DateTime.UtcNow;

        if (existing is null)
        {
            existing = new JobEntity
            {
                JobKey = job.JobKey,
                TenantId = scope.TenantId,
                EnvironmentTag = scope.EnvironmentTag,
                NamespaceSegment = job.Namespace,
                Name = job.Name,
                Variant = job.Variant,
                CreatedAtUtc = now
            };
            db.Jobs.Add(existing);
        }

        existing.NamespaceSegment = job.Namespace;
        existing.Name = job.Name;
        existing.Variant = job.Variant;
        existing.Description = job.Description;
        existing.MetadataJson = SerializeMetadata(job.Metadata);
        existing.UpdatedAtUtc = now;

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    public async Task<IReadOnlyCollection<JobDefinition>> ListJobsAsync(PartitionScope scope, CancellationToken cancellationToken)
    {

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var rows = await db.Jobs
            .Where(j => j.TenantId == scope.TenantId && j.EnvironmentTag == scope.EnvironmentTag)
            .OrderBy(j => j.NamespaceSegment)
            .ThenBy(j => j.Name)
            .ThenBy(j => j.Variant)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        var result = new List<JobDefinition>(rows.Count);
        foreach (var row in rows)
        {
            result.Add(ToJobDefinition(row));
        }

        return result;
    }

    public async Task<JobDefinition?> GetJobAsync(string jobKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(jobKey)) throw new ArgumentNullException(nameof(jobKey));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.Jobs
            .FirstOrDefaultAsync(j => j.JobKey == jobKey && j.TenantId == scope.TenantId && j.EnvironmentTag == scope.EnvironmentTag, cancellationToken)
            .ConfigureAwait(false);
        return entity is null ? null : ToJobDefinition(entity);
    }

    public async Task DeleteJobAsync(string jobKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(jobKey)) throw new ArgumentNullException(nameof(jobKey));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.Jobs
            .FirstOrDefaultAsync(j => j.JobKey == jobKey && j.TenantId == scope.TenantId && j.EnvironmentTag == scope.EnvironmentTag, cancellationToken)
            .ConfigureAwait(false);
        if (entity is null)
        {
            return;
        }

        db.Jobs.Remove(entity);
        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    public async Task UpsertTriggerAsync(TriggerDefinition trigger, CancellationToken cancellationToken)
    {
        if (trigger is null) throw new ArgumentNullException(nameof(trigger));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var job = await db.Jobs
            .FirstOrDefaultAsync(j => j.JobKey == trigger.JobKey && j.TenantId == trigger.Scope.TenantId && j.EnvironmentTag == trigger.Scope.EnvironmentTag, cancellationToken)
            .ConfigureAwait(false)
            ?? throw new InvalidOperationException($"Job '{trigger.JobKey}' must be created before triggers can be added.");

        var timeZoneId = ResolveTimeZoneId(trigger.TimeZoneId, trigger.TriggerId);
        timeZoneId = ResolveTimeZone(timeZoneId).Id;
        var nextFire = ComputeNextFireUtc(
            trigger.ScheduleExpression,
            timeZoneId,
            trigger.StartAtUtc?.UtcDateTime,
            trigger.EndAtUtc?.UtcDateTime,
            DateTimeOffset.UtcNow);
        if (nextFire is null)
        {
            throw new InvalidOperationException($"Cron expression '{trigger.ScheduleExpression}' produced no future occurrences.");
        }

        var entity = await db.Triggers.FirstOrDefaultAsync(t => t.TriggerKey == trigger.TriggerId, cancellationToken).ConfigureAwait(false);
        var now = DateTime.UtcNow;
        if (entity is null)
        {
            entity = new TriggerEntity
            {
                TriggerKey = trigger.TriggerId,
                JobKey = trigger.JobKey,
                JobId = job.Id,
                CreatedAtUtc = now
            };
            db.Triggers.Add(entity);
        }

        entity.JobId = job.Id;
        entity.JobKey = trigger.JobKey;
        entity.CronExpression = trigger.ScheduleExpression;
        entity.TimeZoneId = timeZoneId;
        entity.StartAtUtc = trigger.StartAtUtc?.UtcDateTime;
        entity.EndAtUtc = trigger.EndAtUtc?.UtcDateTime;
        entity.NextFireAtUtc = nextFire.Value;
        entity.Enabled = trigger.Enabled;
        entity.MetadataJson = SerializeMetadata(trigger.Metadata);
        entity.IsDeleted = false;
        entity.UpdatedAtUtc = now;
        entity.LastResult = null;

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    public async Task<IReadOnlyCollection<TriggerDefinition>> ListTriggersAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var rows = await db.Triggers
            .Include(t => t.Job)
            .Where(t => !t.IsDeleted
                        && t.Job.TenantId == scope.TenantId
                        && t.Job.EnvironmentTag == scope.EnvironmentTag)
            .ToListAsync(cancellationToken).ConfigureAwait(false);

        var result = new List<TriggerDefinition>(rows.Count);
        foreach (var row in rows)
        {
            var triggerScope = new PartitionScope(row.Job.TenantId, row.Job.EnvironmentTag);
            result.Add(new TriggerDefinition(
                row.TriggerKey,
                row.JobKey,
                row.CronExpression,
                triggerScope,
                row.StartAtUtc is null ? null : new DateTimeOffset(DateTime.SpecifyKind(row.StartAtUtc.Value, DateTimeKind.Utc)),
                row.EndAtUtc is null ? null : new DateTimeOffset(DateTime.SpecifyKind(row.EndAtUtc.Value, DateTimeKind.Utc)),
                row.Enabled,
                DeserializeMetadata(row.MetadataJson),
                row.TimeZoneId));
        }

        return result;
    }

    public async Task DeleteTriggerAsync(string triggerId, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(triggerId)) throw new ArgumentNullException(nameof(triggerId));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.Triggers.Include(t => t.Job)
            .FirstOrDefaultAsync(t => t.TriggerKey == triggerId, cancellationToken)
            .ConfigureAwait(false);

        if (entity is null)
        {
            return;
        }

        if (!string.Equals(entity.Job.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            || !string.Equals(entity.Job.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException("Trigger scope does not match requested scope.");
        }

        entity.IsDeleted = true;
        entity.Enabled = false;
        entity.NextFireAtUtc = null;
        entity.LeaseId = null;
        entity.LeaseInstanceId = null;
        entity.LeaseExpiresAtUtc = null;
        entity.UpdatedAtUtc = DateTime.UtcNow;

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    public async Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var strategy = db.Database.CreateExecutionStrategy();

        return await strategy.ExecuteAsync(async () =>
        {
            await using var tx = await db.Database.BeginTransactionAsync(IsolationLevel.Serializable, cancellationToken).ConfigureAwait(false);

            var nowUtc = request.NowUtc.UtcDateTime;
            var expiresAt = nowUtc.AddSeconds(_options.LeaseDurationSeconds);

            var due = await db.Triggers
                .Include(t => t.Job)
                .Where(t => !t.IsDeleted && t.Enabled)
                .Where(t => t.Job.TenantId == request.Scope.TenantId && t.Job.EnvironmentTag == request.Scope.EnvironmentTag)
                .Where(t => t.NextFireAtUtc != null && t.NextFireAtUtc <= nowUtc)
                .Where(t => t.LeaseExpiresAtUtc == null || t.LeaseExpiresAtUtc <= nowUtc)
                .OrderBy(t => t.NextFireAtUtc)
                .ThenBy(t => t.Id)
                .Take(request.BatchSize)
                .ToListAsync(cancellationToken).ConfigureAwait(false);

            var leases = new List<TriggerLease>(due.Count);
            foreach (var trigger in due)
            {
                var leaseId = Guid.NewGuid().ToString("N");
                trigger.LeaseId = leaseId;
                trigger.LeaseInstanceId = request.InstanceId;
                trigger.LeaseExpiresAtUtc = expiresAt;
                trigger.LastFiredAtUtc = trigger.NextFireAtUtc;
                trigger.UpdatedAtUtc = nowUtc;

                var scope = new PartitionScope(trigger.Job.TenantId, trigger.Job.EnvironmentTag);
                var fireAt = trigger.NextFireAtUtc ?? nowUtc;

                leases.Add(new TriggerLease(
                    leaseId,
                    trigger.TriggerKey,
                    trigger.JobKey,
                    scope,
                    new DateTimeOffset(DateTime.SpecifyKind(fireAt, DateTimeKind.Utc)),
                    new DateTimeOffset(DateTime.SpecifyKind(trigger.LeaseExpiresAtUtc!.Value, DateTimeKind.Utc)),
                    trigger.MetadataJson));
            }

            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
            await tx.CommitAsync(cancellationToken).ConfigureAwait(false);

            return (IReadOnlyCollection<TriggerLease>)leases;
        }).ConfigureAwait(false);
    }

    public async Task<TriggerLease?> TryRenewLeaseAsync(TriggerLeaseRenewRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var trigger = await db.Triggers.FirstOrDefaultAsync(t => t.TriggerKey == request.Lease.TriggerId, cancellationToken).ConfigureAwait(false);
        if (trigger is null)
        {
            return null;
        }

        if (!string.Equals(trigger.LeaseId, request.Lease.LeaseId, StringComparison.OrdinalIgnoreCase))
        {
            return null;
        }

        if (!string.Equals(trigger.LeaseInstanceId, request.InstanceId, StringComparison.OrdinalIgnoreCase))
        {
            return null;
        }

        var nowUtc = request.NowUtc.UtcDateTime;
        if (!trigger.LeaseExpiresAtUtc.HasValue || trigger.LeaseExpiresAtUtc <= nowUtc)
        {
            return null;
        }

        var expiresAt = nowUtc.AddSeconds(_options.LeaseDurationSeconds);
        trigger.LeaseExpiresAtUtc = expiresAt;
        trigger.UpdatedAtUtc = nowUtc;

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);

        return request.Lease with { LeaseExpiresAtUtc = new DateTimeOffset(DateTime.SpecifyKind(expiresAt, DateTimeKind.Utc)) };
    }

    public async Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var trigger = await db.Triggers.FirstOrDefaultAsync(t => t.TriggerKey == request.Lease.TriggerId, cancellationToken).ConfigureAwait(false)
            ?? throw new InvalidOperationException($"Trigger '{request.Lease.TriggerId}' not found.");

        if (!string.Equals(trigger.LeaseId, request.Lease.LeaseId, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException($"Lease '{request.Lease.LeaseId}' is not active for trigger '{request.Lease.TriggerId}'.");
        }

        if (!string.Equals(trigger.LeaseInstanceId, request.InstanceId, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException($"Lease '{request.Lease.LeaseId}' is owned by another instance.");
        }

        var nowUtc = DateTime.UtcNow;
        var deadLetterReason = string.IsNullOrWhiteSpace(request.DeadLetterReason)
            ? null
            : TruncateReason(request.DeadLetterReason);

        trigger.LeaseId = null;
        trigger.LeaseInstanceId = null;
        trigger.LeaseExpiresAtUtc = null;
        trigger.LastCompletedAtUtc = nowUtc;
        trigger.LastResult = deadLetterReason;

        if (!string.IsNullOrWhiteSpace(deadLetterReason))
        {
            db.DeadLetters.Add(new DeadLetterEntity
            {
                TriggerId = trigger.Id,
                FireAtUtc = request.Lease.FireAtUtc.UtcDateTime,
                Reason = deadLetterReason,
                Payload = request.Lease.Payload ?? string.Empty,
                MetadataJson = null,
                CreatedAtUtc = nowUtc,
                ExpiresAtUtc = nowUtc.AddDays(_options.DeadLetterRetentionDays)
            });
        }

        if (TriggerSchedule.IsOnceExpression(trigger.CronExpression))
        {
            if (request.NextFireTimeUtc.HasValue)
            {
                trigger.NextFireAtUtc = request.NextFireTimeUtc.Value.UtcDateTime;
                trigger.Enabled = true;
            }
            else
            {
                trigger.Enabled = false;
                trigger.NextFireAtUtc = null;
            }

            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
            return;
        }

        if (!request.Succeeded && request.NextFireTimeUtc is null)
        {
            trigger.Enabled = false;
            trigger.NextFireAtUtc = null;
        }
        else
        {
            var next = request.NextFireTimeUtc?.UtcDateTime
                ?? ComputeNextFireUtc(
                    trigger.CronExpression,
                    trigger.TimeZoneId,
                    trigger.StartAtUtc,
                    trigger.EndAtUtc,
                    request.Lease.FireAtUtc);
            trigger.NextFireAtUtc = next;
            if (next is null)
            {
                trigger.Enabled = false;
            }
        }

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    public async Task MoveToDeadLetterAsync(DeadLetterRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var trigger = await db.Triggers.FirstOrDefaultAsync(t => t.TriggerKey == request.Lease.TriggerId, cancellationToken).ConfigureAwait(false)
            ?? throw new InvalidOperationException($"Trigger '{request.Lease.TriggerId}' not found.");

        var reason = TruncateReason(request.Reason);
        var expiresAt = request.OccurredAtUtc.UtcDateTime.AddDays(Math.Clamp(request.Retention.TotalDays <= 0 ? 1 : request.Retention.TotalDays, 1, _options.DeadLetterRetentionDays));

        var deadLetter = new DeadLetterEntity
        {
            TriggerId = trigger.Id,
            FireAtUtc = request.Lease.FireAtUtc.UtcDateTime,
            Reason = reason,
            Payload = BuildDeadLetterPayload(request),
            MetadataJson = SerializeMetadata(request.Metadata),
            CreatedAtUtc = DateTime.UtcNow,
            ExpiresAtUtc = expiresAt
        };

        db.DeadLetters.Add(deadLetter);
        trigger.LastResult = reason;
        trigger.UpdatedAtUtc = DateTime.UtcNow;

        await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
    }

    private string BuildDeadLetterPayload(DeadLetterRequest request)
    {
        var envelope = new DeadLetterEnvelope(
            request.Lease.JobKey,
            request.Lease.TriggerId,
            request.Lease.LeaseId,
            request.Lease.FireAtUtc,
            request.OccurredAtUtc,
            request.Lease.Payload,
            request.Payload,
            request.Metadata,
            request.Reason);

        return JsonSerializer.Serialize(envelope, _jsonOptions);
    }

    private string? SerializeMetadata(IReadOnlyDictionary<string, string>? metadata)
    {
        if (metadata is null || metadata.Count == 0) return null;
        return JsonSerializer.Serialize(metadata, _jsonOptions);
    }

    private IReadOnlyDictionary<string, string>? DeserializeMetadata(string? metadataJson)
    {
        if (string.IsNullOrWhiteSpace(metadataJson)) return null;
        return JsonSerializer.Deserialize<Dictionary<string, string>>(metadataJson, _jsonOptions);
    }

    private string TruncateReason(string reason)
    {
        if (string.IsNullOrWhiteSpace(reason))
        {
            return "unknown";
        }

        var max = _options.DeadLetterReasonMaxLength;
        return reason.Length <= max ? reason : reason[..max];
    }

    private DateTime? ComputeNextFireUtc(string cronExpression, string timeZoneId, DateTime? startAtUtc, DateTime? endAtUtc, DateTimeOffset referenceUtc)
    {
        var start = startAtUtc.HasValue
            ? new DateTimeOffset(DateTime.SpecifyKind(startAtUtc.Value, DateTimeKind.Utc))
            : (DateTimeOffset?)null;
        var end = endAtUtc.HasValue
            ? new DateTimeOffset(DateTime.SpecifyKind(endAtUtc.Value, DateTimeKind.Utc))
            : (DateTimeOffset?)null;

        var next = TriggerSchedule.GetNextOccurrence(
            cronExpression,
            referenceUtc,
            start,
            end,
            ResolveTimeZone(timeZoneId));

        return next?.UtcDateTime;
    }

    private static TimeZoneInfo ResolveTimeZone(string timeZoneId)
    {
        if (string.IsNullOrWhiteSpace(timeZoneId))
        {
            return TimeZoneInfo.Utc;
        }

        try
        {
            return TimeZoneInfo.FindSystemTimeZoneById(timeZoneId);
        }
        catch
        {
            return TimeZoneInfo.Utc;
        }
    }

    private JobDefinition ToJobDefinition(JobEntity entity)
    {
        return new JobDefinition(
            entity.JobKey,
            entity.NamespaceSegment,
            entity.Name,
            entity.Variant,
            entity.Description,
            DeserializeMetadata(entity.MetadataJson));
    }

    private sealed record DeadLetterEnvelope(
        string JobKey,
        string TriggerId,
        string LeaseId,
        DateTimeOffset FireAtUtc,
        DateTimeOffset OccurredAtUtc,
        string? TriggerPayload,
        string? Payload,
        IReadOnlyDictionary<string, string>? Metadata,
        string Reason);

    private static string ResolveTimeZoneId(string? explicitTimeZoneId, string triggerKey)
    {
        if (!string.IsNullOrWhiteSpace(explicitTimeZoneId))
        {
            return explicitTimeZoneId.Trim();
        }

        var extracted = TryExtractTimeZone(triggerKey);
        return string.IsNullOrWhiteSpace(extracted) ? "UTC" : extracted;
    }

    private static string? TryExtractTimeZone(string triggerKey)
    {
        if (string.IsNullOrWhiteSpace(triggerKey))
        {
            return null;
        }

        var parts = triggerKey.Split(':');
        return parts.Length > 6 ? parts[6] : null;
    }
}
