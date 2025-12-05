using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Text.Json;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Croniq.Persistence.Xtraq.Core;
using Croniq.Persistence.Xtraq.Croniq;
using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Persistence.Xtraq;

/// <summary>
/// Xtraq-backed implementation of the job persistence provider.
/// Generated Xtraq artefacts should be placed under <c>Xtraq/Generated</c> in this project
/// and wired up here once available.
/// </summary>
public sealed class XtraqJobPersistenceProvider : IJobPersistenceProvider
{
    private readonly ILogger<XtraqJobPersistenceProvider> _logger;
    private readonly XtraqOptions _options;
    private readonly XtraqDbContext _db;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);
    private readonly ConcurrentDictionary<string, LeaseContext> _leaseContexts = new();
    private string? _currentInstanceId;
    private readonly int _leaseDurationSeconds;
    private const int DefaultLeaseDurationSeconds = 60;

    public XtraqJobPersistenceProvider(
        XtraqDbContext db,
        IOptions<XtraqOptions> options,
        ILogger<XtraqJobPersistenceProvider> logger)
    {
        _db = db ?? throw new ArgumentNullException(nameof(db));
        _options = options.Value ?? throw new ArgumentNullException(nameof(options));
        _logger = logger ?? throw new ArgumentNullException(nameof(logger));
        _leaseDurationSeconds = _options.LeaseDurationSeconds > 0 ? _options.LeaseDurationSeconds : DefaultLeaseDurationSeconds;
    }

    public Task UpsertJobAsync(JobDefinition job, CancellationToken cancellationToken)
    {
        if (job is null) throw new ArgumentNullException(nameof(job));

        var keyParts = JobKeyParts.Parse(job.JobKey);
        var jobRef = new JobRefRequest
        {
            JobKey = job.JobKey,
            TenantId = keyParts.TenantId,
            Environment = keyParts.Environment,
            Namespace = keyParts.Namespace,
            Name = keyParts.Name,
            Variant = keyParts.Variant,
            Description = job.Description,
            Metadata = SerializeMetadata(job.Metadata)
        };

        var request = new JobUpsertRequest
        {
            Job = new[] { jobRef },
            AllowDeletedReuse = false
        };

        return _db.JobUpsertAsync(request, cancellationToken);
    }

    public Task UpsertTriggerAsync(TriggerDefinition trigger, CancellationToken cancellationToken)
    {
        if (trigger is null) throw new ArgumentNullException(nameof(trigger));

        var triggerKey = trigger.TriggerId;
        var triggerKeyParts = TriggerKeyParts.Parse(triggerKey);
        var jobKeyParts = JobKeyParts.Parse(trigger.JobKey);
        var timeZoneId = triggerKeyParts.TimeZoneId ?? "UTC";

        return Task.Run(async () =>
        {
            var jobId = await ResolveJobIdAsync(jobKeyParts.JobKey, cancellationToken).ConfigureAwait(false);
            var nextFireAtUtc = ComputeNextFireUtc(
                trigger.ScheduleExpression,
                timeZoneId,
                trigger.StartAtUtc?.UtcDateTime,
                trigger.EndAtUtc?.UtcDateTime,
                DateTimeOffset.UtcNow);
            if (nextFireAtUtc is null)
            {
                throw new InvalidOperationException($"Cron expression '{trigger.ScheduleExpression}' produced no future occurrences.");
            }

            var triggerRef = new TriggerRefRequest
            {
                TriggerKey = triggerKey,
                JobKey = jobKeyParts.JobKey,
                TenantId = jobKeyParts.TenantId,
                JobId = jobId,
                Environment = jobKeyParts.Environment,
                Namespace = jobKeyParts.Namespace,
                Name = jobKeyParts.Name,
                Variant = jobKeyParts.Variant,
                CronExpression = trigger.ScheduleExpression,
                TimeZoneId = timeZoneId,
                StartAtUtc = trigger.StartAtUtc?.UtcDateTime,
                EndAtUtc = trigger.EndAtUtc?.UtcDateTime,
                NextFireAtUtc = nextFireAtUtc.Value,
                Enabled = trigger.Enabled,
                Metadata = SerializeMetadata(trigger.Metadata)
            };

            var request = new TriggerUpsertRequest
            {
                Trigger = new[] { triggerRef },
                AllowDeletedReuse = false
            };

            _logger.LogInformation("Upserting trigger {TriggerKey} nextFire={NextFire}", triggerKey, nextFireAtUtc);
            await _db.TriggerUpsertAsync(request, cancellationToken).ConfigureAwait(false);
        }, cancellationToken);
    }

    public Task<IReadOnlyCollection<TriggerDefinition>> ListTriggersAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        throw new NotSupportedException("Listing triggers is not supported by the current Xtraq contract.");
    }

    public Task DeleteTriggerAsync(string triggerId, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(triggerId)) throw new ArgumentNullException(nameof(triggerId));

        return Task.Run(async () =>
        {
            var triggerKeyParts = TriggerKeyParts.Parse(triggerId);
            var jobId = await ResolveJobIdAsync(triggerKeyParts.JobKey, cancellationToken).ConfigureAwait(false);

            var triggerRef = new TriggerRefRequest
            {
                TriggerKey = triggerKeyParts.TriggerKey,
                JobKey = triggerKeyParts.JobKey,
                TenantId = triggerKeyParts.TenantId,
                JobId = jobId,
                Environment = triggerKeyParts.Environment,
                Namespace = triggerKeyParts.Namespace,
                Name = triggerKeyParts.Name,
                Variant = triggerKeyParts.Variant,
                CronExpression = triggerKeyParts.CronExpression ?? string.Empty,
                TimeZoneId = triggerKeyParts.TimeZoneId ?? "UTC",
                StartAtUtc = null,
                EndAtUtc = null,
                NextFireAtUtc = null,
                Enabled = true,
                Metadata = null
            };

            var request = new TriggerDeleteRequest
            {
                Trigger = new[] { triggerRef }
            };

            await _db.TriggerDeleteAsync(request, cancellationToken).ConfigureAwait(false);
        }, cancellationToken);
    }

    public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (string.IsNullOrWhiteSpace(request.InstanceId)) throw new InvalidOperationException("InstanceId is required for lease acquisition.");

        _currentInstanceId = request.InstanceId;
        var leaseDuration = _leaseDurationSeconds > 0 ? _leaseDurationSeconds : DefaultLeaseDurationSeconds;
        var acquire = new TriggerLeaseAcquireRequest
        {
            TenantId = ParseTenantId(request.Scope.TenantId),
            Environment = request.Scope.EnvironmentTag,
            InstanceId = request.InstanceId,
            NowUtc = request.NowUtc.UtcDateTime,
            BatchSize = request.BatchSize,
            LeaseDurationSeconds = leaseDuration
        };

        return Task.Run(async () =>
        {
            var result = await _db.TriggerLeaseAcquireAsync(acquire, cancellationToken).ConfigureAwait(false);
            var leases = new List<TriggerLease>(result.Result.Count);
            foreach (var row in result.Result)
            {
                var leaseId = row.LeaseId.ToString(CultureInfo.InvariantCulture);
                var scope = new PartitionScope(row.TenantId.ToString(CultureInfo.InvariantCulture), row.Environment);
                leases.Add(new TriggerLease(
                    leaseId,
                    row.TriggerKey,
                    row.JobKey,
                    scope,
                    ToUtc(row.FireAtUtc),
                    ToUtc(row.LeaseExpiresAtUtc),
                    row.Payload ?? row.Metadata));

                var ctx = new LeaseContext(
                    row.TriggerKey,
                    row.CronExpression,
                    row.TimeZoneId,
                    row.StartAtUtc,
                    row.EndAtUtc,
                    request.InstanceId);
                _leaseContexts[leaseId] = ctx;
            }

            return (IReadOnlyCollection<TriggerLease>)leases;
        }, cancellationToken);
    }

    public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        return Task.Run(async () =>
        {
            var leaseId = ParseLeaseId(request.Lease.LeaseId);
            var schedule = await ResolveLeaseContextAsync(request.Lease.TriggerId, leaseId, cancellationToken).ConfigureAwait(false);
            var instanceId = schedule.InstanceId ?? _currentInstanceId;
            if (string.IsNullOrWhiteSpace(instanceId))
            {
                throw new InvalidOperationException("InstanceId for lease release could not be determined.");
            }

            var nextFire = request.NextFireTimeUtc?.UtcDateTime
                ?? ComputeNextFireUtc(
                    schedule.CronExpression,
                    schedule.TimeZoneId,
                    schedule.StartAtUtc,
                    schedule.EndAtUtc,
                    request.Lease.FireAtUtc);

            var releaseRef = new TriggerLeaseReleaseRefRequest
            {
                LeaseId = leaseId,
                InstanceId = instanceId,
                Succeeded = request.Succeeded,
                NextFireAtUtc = nextFire,
                DeadLetterReason = request.DeadLetterReason
            };

            var release = new TriggerLeaseReleaseRequest
            {
                Release = new[] { releaseRef }
            };

            _logger.LogInformation("Releasing lease {LeaseId} succeeded={Succeeded} nextFire={NextFire} deadLetter={DeadLetter}",
                leaseId, request.Succeeded, nextFire, request.DeadLetterReason);
            await _db.TriggerLeaseReleaseAsync(release, cancellationToken).ConfigureAwait(false);
            _leaseContexts.TryRemove(request.Lease.LeaseId, out _);
        }, cancellationToken);
    }

    private string? SerializeMetadata(IReadOnlyDictionary<string, string>? metadata)
    {
        if (metadata is null || metadata.Count == 0) return null;
        return JsonSerializer.Serialize(metadata, _jsonOptions);
    }

    private async Task<long> ResolveJobIdAsync(string jobKey, CancellationToken cancellationToken)
    {
        var result = await _db.JobFindByKeyAsync(new JobFindByKeyRequest { JobKey = jobKey }, cancellationToken).ConfigureAwait(false);
        if (result.Result?.JobId is not long id)
        {
            throw new InvalidOperationException($"Job '{jobKey}' not found.");
        }
        return id;
    }

    private static int ParseTenantId(string tenantId)
    {
        if (!int.TryParse(tenantId, NumberStyles.Integer, CultureInfo.InvariantCulture, out var value))
        {
            throw new InvalidOperationException($"TenantId '{tenantId}' must be numeric.");
        }

        return value;
    }

    private static long ParseLeaseId(string leaseId)
    {
        if (!long.TryParse(leaseId, NumberStyles.Integer, CultureInfo.InvariantCulture, out var value))
        {
            throw new InvalidOperationException($"LeaseId '{leaseId}' is not valid.");
        }

        return value;
    }

    private static DateTimeOffset ToUtc(DateTime value) => DateTime.SpecifyKind(value, DateTimeKind.Utc);

    private DateTime? ComputeNextFireUtc(string cronExpression, string timeZoneId, DateTime? startAtUtc, DateTime? endAtUtc, DateTimeOffset referenceUtc)
    {
        var tz = ResolveTimeZone(timeZoneId);
        var schedule = new CronSchedule(cronExpression, tz);
        var cursor = referenceUtc;

        if (startAtUtc.HasValue)
        {
            var start = DateTime.SpecifyKind(startAtUtc.Value, DateTimeKind.Utc);
            if (start > cursor.UtcDateTime)
            {
                cursor = new DateTimeOffset(start, TimeSpan.Zero);
            }
        }

        var next = schedule.GetNextOccurrence(cursor);
        if (next.HasValue && endAtUtc.HasValue)
        {
            var end = DateTime.SpecifyKind(endAtUtc.Value, DateTimeKind.Utc);
            if (next.Value.UtcDateTime > end)
            {
                return null;
            }
        }

        return next?.UtcDateTime;
    }

    private static TimeZoneInfo ResolveTimeZone(string timeZoneId)
    {
        try
        {
            return TimeZoneInfo.FindSystemTimeZoneById(timeZoneId);
        }
        catch
        {
            return TimeZoneInfo.Utc;
        }
    }

    private async Task<LeaseContext> ResolveLeaseContextAsync(string triggerKey, long leaseId, CancellationToken cancellationToken)
    {
        var cacheKey = leaseId.ToString(CultureInfo.InvariantCulture);
        if (_leaseContexts.TryGetValue(cacheKey, out var cached))
        {
            return cached;
        }

        var result = await _db.TriggerFindByKeyAsync(new TriggerFindByKeyRequest { TriggerKey = triggerKey }, cancellationToken).ConfigureAwait(false);
        var row = result.Result.FirstOrDefault();
        if (row.TriggerId == 0)
        {
            throw new InvalidOperationException($"Trigger '{triggerKey}' not found for lease '{leaseId}'.");
        }

        var context = new LeaseContext(
            row.TriggerKey,
            row.CronExpression,
            row.TimeZoneId,
            row.StartAtUtc,
            row.EndAtUtc,
            _currentInstanceId);

        _leaseContexts[cacheKey] = context;
        return context;
    }

    private sealed record LeaseContext(string TriggerKey, string CronExpression, string TimeZoneId, DateTime? StartAtUtc, DateTime? EndAtUtc, string? InstanceId);

    private sealed record JobKeyParts(string JobKey, int TenantId, string Environment, string Namespace, string Name, string? Variant)
    {
        public static JobKeyParts Parse(string key)
        {
            if (string.IsNullOrWhiteSpace(key)) throw new ArgumentNullException(nameof(key));
            var parts = key.Split(':');
            if (parts.Length < 4) throw new InvalidOperationException($"JobKey '{key}' must be formatted as 'tenantId:env:namespace:name[:variant]'.");
            if (!int.TryParse(parts[0], out var tenantId))
            {
                throw new InvalidOperationException($"JobKey '{key}' must start with numeric tenantId.");
            }
            var variant = parts.Length > 4 ? parts[4] : null;
            return new JobKeyParts(key, tenantId, parts[1], parts[2], parts[3], variant);
        }
    }

    private sealed record TriggerKeyParts(string TriggerKey, string JobKey, int TenantId, string Environment, string Namespace, string Name, string? Variant, string? CronExpression, string? TimeZoneId)
    {
        public static TriggerKeyParts Parse(string triggerKey)
        {
            if (string.IsNullOrWhiteSpace(triggerKey)) throw new ArgumentNullException(nameof(triggerKey));
            // Expected format: tenantId:env:namespace:name[:variant][:cron][:tz]
            var parts = triggerKey.Split(':');
            if (parts.Length < 4) throw new InvalidOperationException($"TriggerKey '{triggerKey}' must be formatted as 'tenantId:env:namespace:name[:variant]' at minimum.");
            if (!int.TryParse(parts[0], out var tenantId))
            {
                throw new InvalidOperationException($"TriggerKey '{triggerKey}' must start with numeric tenantId.");
            }

            var variant = parts.Length > 4 ? parts[4] : null;
            var cron = parts.Length > 5 ? parts[5] : null;
            var tz = parts.Length > 6 ? parts[6] : null;
            var jobKey = $"{parts[0]}:{parts[1]}:{parts[2]}:{parts[3]}{(variant is not null ? ":" + variant : string.Empty)}";
            return new TriggerKeyParts(triggerKey, jobKey, tenantId, parts[1], parts[2], parts[3], variant, cron, tz);
        }
    }
}
