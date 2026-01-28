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
using Microsoft.Data.SqlClient;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Persistence.SqlServer;

/// <summary>
/// EF Core backed implementation of <see cref="IJobPersistenceProvider"/>.
/// </summary>
public sealed class SqlServerJobPersistenceProvider : IJobPersistenceProvider, ICalendarStore
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

    private static string NormalizeExecutionMode(string? mode)
    {
        if (string.IsNullOrWhiteSpace(mode))
        {
            return ExecutionIntent.ExecutionModes.Normal;
        }

        return mode.Trim().ToLowerInvariant();
    }

    private static string NormalizeInvocationSource(string? source)
    {
        if (string.IsNullOrWhiteSpace(source))
        {
            return ExecutionIntent.InvocationSources.Schedule;
        }

        return source.Trim().ToLowerInvariant();
    }

    public async Task UpsertJobAsync(JobDefinition job, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (job is null) throw new ArgumentNullException(nameof(job));
        if (!JobKey.TryParse(job.JobKey, out var jobKey))
        {
            throw new InvalidOperationException($"JobKey '{job.JobKey}' is invalid.");
        }

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var now = DateTime.UtcNow;
        var existing = await FindJobAsync(db, job.JobKey, scope, cancellationToken).ConfigureAwait(false);

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

        ApplyJobDefinition(existing, job, now);

        try
        {
            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (DbUpdateException ex) when (IsUniqueConstraintViolation(ex))
        {
            db.ChangeTracker.Clear();
            var retry = await FindJobAsync(db, job.JobKey, scope, cancellationToken).ConfigureAwait(false);
            if (retry is null)
            {
                throw;
            }

            ApplyJobDefinition(retry, job, now);
            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        }
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

    public async Task<CalendarDefinition?> FindAsync(string calendarId, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(calendarId)) throw new ArgumentNullException(nameof(calendarId));
        calendarId = calendarId.Trim();

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await db.Calendars
            .FirstOrDefaultAsync(c => c.CalendarId == calendarId && c.TenantId == scope.TenantId && c.EnvironmentTag == scope.EnvironmentTag, cancellationToken)
            .ConfigureAwait(false);

        return entity is null ? null : ToCalendarDefinition(entity);
    }

    public async Task<IReadOnlyCollection<CalendarDefinition>> ListCalendarsAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var rows = await db.Calendars
            .Where(c => c.TenantId == scope.TenantId && c.EnvironmentTag == scope.EnvironmentTag)
            .OrderBy(c => c.Name)
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        var result = new List<CalendarDefinition>(rows.Count);
        foreach (var row in rows)
        {
            result.Add(ToCalendarDefinition(row));
        }

        return result;
    }

    public async Task UpsertAsync(CalendarUpsert request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        var scope = new PartitionScope(request.TenantId, request.EnvironmentTag);
        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var now = DateTime.UtcNow;
        var existing = await FindCalendarEntityAsync(db, request.CalendarId, scope, cancellationToken).ConfigureAwait(false);

        if (existing is null)
        {
            existing = new CalendarEntity
            {
                CalendarId = request.CalendarId,
                TenantId = request.TenantId,
                EnvironmentTag = request.EnvironmentTag,
                CreatedAtUtc = now
            };
            db.Calendars.Add(existing);
        }

        ApplyCalendarDefinition(existing, request, now);

        try
        {
            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (DbUpdateException ex) when (IsUniqueConstraintViolation(ex))
        {
            db.ChangeTracker.Clear();
            var retry = await FindCalendarEntityAsync(db, request.CalendarId, scope, cancellationToken).ConfigureAwait(false);
            if (retry is null)
            {
                throw;
            }

            ApplyCalendarDefinition(retry, request, now);
            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        }
    }

    public async Task DeleteAsync(string calendarId, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(calendarId)) throw new ArgumentNullException(nameof(calendarId));
        calendarId = calendarId.Trim();

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        var entity = await FindCalendarEntityAsync(db, calendarId, scope, cancellationToken).ConfigureAwait(false);
        if (entity is null)
        {
            return;
        }

        db.Calendars.Remove(entity);
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

        var calendarId = NormalizeCalendarId(trigger.CalendarId);
        var calendar = await GetRequiredCalendarAsync(db, calendarId, trigger.Scope, trigger.TriggerId, cancellationToken).ConfigureAwait(false);
        var timeZoneId = ResolveTimeZoneId(trigger.TimeZoneId, trigger.TriggerId);
        timeZoneId = ResolveTimeZone(timeZoneId).Id;
        var normalizedTrigger = trigger with { TimeZoneId = timeZoneId, CalendarId = calendarId };
        var nextFire = ComputeNextFire(normalizedTrigger, calendar, DateTimeOffset.UtcNow);

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
        entity.CalendarId = calendarId;
        entity.StartAtUtc = trigger.StartAtUtc?.UtcDateTime;
        entity.EndAtUtc = trigger.EndAtUtc?.UtcDateTime;
        entity.NextFireAtUtc = nextFire?.UtcDateTime;
        entity.Enabled = trigger.Enabled;
        entity.MetadataJson = SerializeMetadata(trigger.Metadata);
        entity.ExecutionMode = NormalizeExecutionMode(trigger.ExecutionMode);
        entity.InvocationSource = NormalizeInvocationSource(trigger.InvocationSource);
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
                row.TimeZoneId,
                row.CalendarId,
                row.ExecutionMode,
                row.InvocationSource));
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
                .Where(t => t.Job.IsActive)
                .Where(t => t.Job.AssignedRunnerId == request.InstanceId)
                .Where(t => request.AllowTestExecutions || t.ExecutionMode == null || t.ExecutionMode != ExecutionIntent.ExecutionModes.Test)
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
                var executionId = Guid.NewGuid().ToString("N");
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
                    trigger.MetadataJson,
                    executionId,
                    trigger.ExecutionMode,
                    trigger.InvocationSource));
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
        var trigger = await db.Triggers.Include(t => t.Job)
            .FirstOrDefaultAsync(t => t.TriggerKey == request.Lease.TriggerId, cancellationToken).ConfigureAwait(false)
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
                var nextFireUtc = request.NextFireTimeUtc.Value.UtcDateTime;
                trigger.StartAtUtc = nextFireUtc;
                trigger.NextFireAtUtc = nextFireUtc;
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
            trigger.NextFireAtUtc = null;
            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
            return;
        }

        var scope = new PartitionScope(trigger.Job.TenantId, trigger.Job.EnvironmentTag);
        var calendar = await ResolveCalendarAsync(db, trigger.CalendarId, scope, trigger.TriggerKey, cancellationToken).ConfigureAwait(false);
        var evaluationTrigger = BuildTriggerDefinition(trigger, scope);
        var next = request.NextFireTimeUtc?.UtcDateTime
            ?? ComputeNextFire(evaluationTrigger, calendar, request.Lease.FireAtUtc)?.UtcDateTime;
        trigger.NextFireAtUtc = next;

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

    private string? SerializeRules(IReadOnlyCollection<CalendarRuleDefinition>? rules)
    {
        if (rules is null || rules.Count == 0) return null;
        return JsonSerializer.Serialize(rules, _jsonOptions);
    }

    private IReadOnlyCollection<CalendarRuleDefinition> DeserializeRules(string? rulesJson)
    {
        if (string.IsNullOrWhiteSpace(rulesJson)) return Array.Empty<CalendarRuleDefinition>();
        var rules = JsonSerializer.Deserialize<List<CalendarRuleDefinition>>(rulesJson, _jsonOptions);
        return rules is null ? Array.Empty<CalendarRuleDefinition>() : rules;
    }

    private static async Task<JobEntity?> FindJobAsync(
        SqlServerDbContext db,
        string jobKey,
        PartitionScope scope,
        CancellationToken cancellationToken)
    {
        return await db.Jobs.FirstOrDefaultAsync(
            j => j.JobKey == jobKey
                 && j.TenantId == scope.TenantId
                 && j.EnvironmentTag == scope.EnvironmentTag,
            cancellationToken).ConfigureAwait(false);
    }

    private static async Task<CalendarEntity?> FindCalendarEntityAsync(
        SqlServerDbContext db,
        string calendarId,
        PartitionScope scope,
        CancellationToken cancellationToken)
    {
        return await db.Calendars.FirstOrDefaultAsync(
            c => c.CalendarId == calendarId
                 && c.TenantId == scope.TenantId
                 && c.EnvironmentTag == scope.EnvironmentTag,
            cancellationToken).ConfigureAwait(false);
    }

    private void ApplyJobDefinition(JobEntity entity, JobDefinition job, DateTime now)
    {
        entity.NamespaceSegment = job.Namespace;
        entity.Name = job.Name;
        entity.Variant = job.Variant;
        entity.Description = job.Description;
        entity.IsActive = job.IsActive;
        entity.AssignedRunnerId = job.AssignedRunnerId;
        entity.AssignedBy = job.AssignedBy;
        entity.AssignedAtUtc = job.AssignedAtUtc?.UtcDateTime;
        entity.AssignmentSource = job.AssignmentSource;
        entity.AssignmentNotes = job.AssignmentNotes;
        entity.MetadataJson = SerializeMetadata(job.Metadata);
        entity.UpdatedAtUtc = now;
    }

    private void ApplyCalendarDefinition(CalendarEntity entity, CalendarUpsert request, DateTime now)
    {
        entity.Name = request.Name;
        entity.Description = request.Description;
        entity.TimeZoneId = request.TimeZoneId;
        entity.Mode = (int)request.Mode;
        entity.RulesJson = SerializeRules(request.Rules);
        entity.Enabled = request.Enabled;
        entity.UpdatedAtUtc = now;
    }

    private CalendarMode ResolveCalendarMode(int mode)
    {
        if (Enum.IsDefined(typeof(CalendarMode), mode))
        {
            return (CalendarMode)mode;
        }

        _logger.LogWarning("Calendar mode value {Mode} is invalid; defaulting to Include.", mode);
        return CalendarMode.Include;
    }

    private CalendarDefinition ToCalendarDefinition(CalendarEntity entity)
    {
        return new CalendarDefinition(
            entity.CalendarId,
            entity.TenantId,
            entity.EnvironmentTag,
            entity.Name,
            entity.Description,
            entity.TimeZoneId,
            ResolveCalendarMode(entity.Mode),
            DeserializeRules(entity.RulesJson),
            entity.Enabled,
            ToUtcOffset(entity.CreatedAtUtc),
            ToUtcOffset(entity.UpdatedAtUtc));
    }

    private static DateTimeOffset ToUtcOffset(DateTime value)
    {
        return new DateTimeOffset(DateTime.SpecifyKind(value, DateTimeKind.Utc));
    }

    private static bool IsUniqueConstraintViolation(DbUpdateException exception)
    {
        if (exception.InnerException is not SqlException sqlException)
        {
            return false;
        }

        foreach (SqlError error in sqlException.Errors)
        {
            if (error.Number is 2601 or 2627)
            {
                return true;
            }
        }

        return false;
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

    private static string? NormalizeCalendarId(string? calendarId)
    {
        return string.IsNullOrWhiteSpace(calendarId) ? null : calendarId.Trim();
    }

    private async Task<CalendarDefinition?> GetRequiredCalendarAsync(
        SqlServerDbContext db,
        string? calendarId,
        PartitionScope scope,
        string triggerId,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(calendarId))
        {
            return null;
        }

        var entity = await FindCalendarEntityAsync(db, calendarId, scope, cancellationToken).ConfigureAwait(false);
        if (entity is null)
        {
            throw new InvalidOperationException($"Calendar '{calendarId}' not found for trigger '{triggerId}'.");
        }

        return ToCalendarDefinition(entity);
    }

    private async Task<CalendarDefinition?> ResolveCalendarAsync(
        SqlServerDbContext db,
        string? calendarId,
        PartitionScope scope,
        string triggerId,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(calendarId))
        {
            return null;
        }

        var entity = await FindCalendarEntityAsync(db, calendarId, scope, cancellationToken).ConfigureAwait(false);
        if (entity is null)
        {
            _logger.LogWarning(
                "Calendar {CalendarId} not found for trigger {TriggerId}; continuing without calendar filtering.",
                calendarId,
                triggerId);
            return null;
        }

        return ToCalendarDefinition(entity);
    }

    private static TriggerDefinition BuildTriggerDefinition(TriggerEntity trigger, PartitionScope scope)
    {
        return new TriggerDefinition(
            trigger.TriggerKey,
            trigger.JobKey,
            trigger.CronExpression,
            scope,
            trigger.StartAtUtc is null ? null : ToUtcOffset(trigger.StartAtUtc.Value),
            trigger.EndAtUtc is null ? null : ToUtcOffset(trigger.EndAtUtc.Value),
            trigger.Enabled,
            null,
            trigger.TimeZoneId,
            trigger.CalendarId,
            trigger.ExecutionMode,
            trigger.InvocationSource);
    }

    private DateTimeOffset? ComputeNextFire(TriggerDefinition trigger, CalendarDefinition? calendar, DateTimeOffset referenceUtc)
    {
        if (calendar is not null && calendar.Enabled)
        {
            return CalendarEvaluator.GetNextOccurrence(trigger, referenceUtc, calendar, _options.CalendarEvaluation, _logger);
        }

        return TriggerSchedule.GetNextOccurrence(
            trigger.ScheduleExpression,
            referenceUtc,
            trigger.StartAtUtc,
            trigger.EndAtUtc,
            ResolveTimeZone(trigger.TimeZoneId));
    }

    private static TimeZoneInfo ResolveTimeZone(string? timeZoneId)
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
            DeserializeMetadata(entity.MetadataJson),
            entity.IsActive,
            entity.AssignedRunnerId,
            entity.AssignedBy,
            entity.AssignedAtUtc is null ? null : ToUtcOffset(entity.AssignedAtUtc.Value),
            entity.AssignmentSource,
            entity.AssignmentNotes);
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
