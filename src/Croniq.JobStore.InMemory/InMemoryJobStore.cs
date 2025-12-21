using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Jobs;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Options;

namespace Croniq.JobStore.InMemory;

/// <summary>
/// Reference in-memory implementation of the persistence abstractions.
/// </summary>
public sealed class InMemoryJobStore : IJobPersistenceProvider, IJobDeadLetterStore
{
    private readonly object _lock = new();
    private readonly Dictionary<string, JobDefinition> _jobs = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, TriggerEntry> _triggers = new(StringComparer.OrdinalIgnoreCase);
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);
    private readonly InMemoryJobStoreOptions _options;
    private long _leaseSequence;
    private long _deadLetterSequence;

    public InMemoryJobStore(IOptions<InMemoryJobStoreOptions>? options = null)
    {
        _options = options?.Value ?? new InMemoryJobStoreOptions();
        _options.Normalize();
    }

    public Task UpsertJobAsync(JobDefinition job, CancellationToken cancellationToken)
    {
        if (job is null) throw new ArgumentNullException(nameof(job));
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            _jobs[job.JobKey] = CloneJob(job);
        }

        return Task.CompletedTask;
    }

    public Task<IReadOnlyCollection<JobDefinition>> ListJobsAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            var matches = _jobs.Values
                .Where(job => JobMatchesScope(job.JobKey, scope))
                .Select(CloneJob)
                .ToArray();

            return Task.FromResult<IReadOnlyCollection<JobDefinition>>(matches);
        }
    }

    public Task<JobDefinition?> GetJobAsync(string jobKey, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(jobKey)) throw new ArgumentNullException(nameof(jobKey));
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            if (!_jobs.TryGetValue(jobKey, out var job))
            {
                return Task.FromResult<JobDefinition?>(null);
            }

            return Task.FromResult<JobDefinition?>(CloneJob(job));
        }
    }

    public Task DeleteJobAsync(string jobKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(jobKey)) throw new ArgumentNullException(nameof(jobKey));
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            if (!_jobs.TryGetValue(jobKey, out var existing) || !JobMatchesScope(existing.JobKey, scope))
            {
                return Task.CompletedTask;
            }

            _jobs.Remove(jobKey);

            var triggerKeys = _triggers
                .Where(pair => string.Equals(pair.Value.Definition.JobKey, jobKey, StringComparison.OrdinalIgnoreCase))
                .Select(pair => pair.Key)
                .ToList();

            foreach (var key in triggerKeys)
            {
                _triggers.Remove(key);
            }
        }

        return Task.CompletedTask;
    }

    public Task UpsertTriggerAsync(TriggerDefinition trigger, CancellationToken cancellationToken)
    {
        if (trigger is null) throw new ArgumentNullException(nameof(trigger));
        cancellationToken.ThrowIfCancellationRequested();

        var isOnce = TriggerSchedule.IsOnceExpression(trigger.ScheduleExpression);
        var timeZone = ResolveTimeZone(trigger.TimeZoneId);
        var schedule = isOnce ? null : new CronSchedule(trigger.ScheduleExpression, timeZone);
        var now = UtcNow();
        var nextFire = ComputeNextFire(trigger, schedule, now);

        lock (_lock)
        {
            _triggers[trigger.TriggerId] = new TriggerEntry(trigger, schedule, nextFire);
        }

        return Task.CompletedTask;
    }

    public Task<IReadOnlyCollection<TriggerDefinition>> ListTriggersAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            var result = _triggers.Values
                .Where(t => MatchesScope(t.Definition.Scope, scope))
                .Select(t => t.Definition)
                .ToArray();

            return Task.FromResult<IReadOnlyCollection<TriggerDefinition>>(result);
        }
    }

    public Task DeleteTriggerAsync(string triggerId, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(triggerId)) throw new ArgumentNullException(nameof(triggerId));
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            if (_triggers.TryGetValue(triggerId, out var entry) && MatchesScope(entry.Definition.Scope, scope))
            {
                _triggers.Remove(triggerId);
            }
        }

        return Task.CompletedTask;
    }

    public Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        cancellationToken.ThrowIfCancellationRequested();

        var leases = new List<TriggerLease>();
        lock (_lock)
        {
            var now = request.NowUtc;
            var leaseDuration = TimeSpan.FromSeconds(_options.LeaseDurationSeconds);

            var candidates = _triggers.Values
                .Where(t => MatchesScope(t.Definition.Scope, request.Scope) && t.Definition.Enabled)
                .OrderBy(t => t.NextFireAtUtc ?? DateTimeOffset.MaxValue);

            foreach (var entry in candidates)
            {
                if (entry.Lease is { } active && active.ExpiresAtUtc > now)
                {
                    continue;
                }

                if (entry.Lease is not null && entry.Lease.ExpiresAtUtc <= now)
                {
                    entry.Lease = null;
                }

                if (entry.NextFireAtUtc is null || entry.NextFireAtUtc > now)
                {
                    continue;
                }

                var leaseId = Interlocked.Increment(ref _leaseSequence).ToString();
                var expiresAt = now.Add(leaseDuration);
                entry.Lease = new LeaseInfo(leaseId, request.InstanceId, expiresAt, entry.NextFireAtUtc.Value);

                leases.Add(new TriggerLease(
                    leaseId,
                    entry.Definition.TriggerId,
                    entry.Definition.JobKey,
                    entry.Definition.Scope,
                    entry.NextFireAtUtc.Value,
                    expiresAt,
                    SerializeMetadata(entry.Definition.Metadata)));

                if (leases.Count >= request.BatchSize)
                {
                    break;
                }
            }
        }

        return Task.FromResult<IReadOnlyCollection<TriggerLease>>(leases);
    }

    public Task<TriggerLease?> TryRenewLeaseAsync(TriggerLeaseRenewRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            if (!_triggers.TryGetValue(request.Lease.TriggerId, out var entry))
            {
                return Task.FromResult<TriggerLease?>(null);
            }

            if (entry.Lease is null || !string.Equals(entry.Lease.LeaseId, request.Lease.LeaseId, StringComparison.Ordinal))
            {
                return Task.FromResult<TriggerLease?>(null);
            }

            if (!string.Equals(entry.Lease.InstanceId, request.InstanceId, StringComparison.Ordinal))
            {
                return Task.FromResult<TriggerLease?>(null);
            }

            var now = request.NowUtc;
            if (entry.Lease.ExpiresAtUtc <= now)
            {
                entry.Lease = null;
                return Task.FromResult<TriggerLease?>(null);
            }

            var leaseDuration = TimeSpan.FromSeconds(_options.LeaseDurationSeconds);
            var expiresAt = now.Add(leaseDuration);
            entry.Lease = entry.Lease with { ExpiresAtUtc = expiresAt };

            var renewed = request.Lease with { LeaseExpiresAtUtc = expiresAt };
            return Task.FromResult<TriggerLease?>(renewed);
        }
    }

    public Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            if (!_triggers.TryGetValue(request.Lease.TriggerId, out var entry))
            {
                throw new InvalidOperationException($"Trigger '{request.Lease.TriggerId}' not found.");
            }

            if (entry.Lease is null || !string.Equals(entry.Lease.LeaseId, request.Lease.LeaseId, StringComparison.Ordinal))
            {
                throw new InvalidOperationException($"Lease '{request.Lease.LeaseId}' is not active for trigger '{request.Lease.TriggerId}'.");
            }

            if (!string.IsNullOrWhiteSpace(request.DeadLetterReason))
            {
                var id = Interlocked.Increment(ref _deadLetterSequence);
                entry.DeadLetters.Add(new DeadLetterEntry(
                    Id: id,
                    TriggerId: request.Lease.TriggerId,
                    JobKey: request.Lease.JobKey,
                    Scope: request.Lease.Scope,
                    FireAtUtc: request.Lease.FireAtUtc,
                    Reason: request.DeadLetterReason!,
                    Payload: request.Lease.Payload ?? string.Empty,
                    Metadata: null,
                    CreatedAtUtc: UtcNow(),
                    ExpiresAtUtc: null));
            }

            entry.Lease = null;

            if (TriggerSchedule.IsOnceExpression(entry.Definition.ScheduleExpression))
            {
                entry.NextFireAtUtc = request.NextFireTimeUtc;
                return Task.CompletedTask;
            }

            if (!request.Succeeded && request.NextFireTimeUtc is null)
            {
                entry.NextFireAtUtc = null;
                return Task.CompletedTask;
            }

            entry.NextFireAtUtc = request.NextFireTimeUtc ?? ComputeNextFire(entry.Definition, entry.Schedule, request.Lease.FireAtUtc);
        }

        return Task.CompletedTask;
    }

    public Task MoveToDeadLetterAsync(DeadLetterRequest request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            if (!_triggers.TryGetValue(request.Lease.TriggerId, out var entry))
            {
                throw new InvalidOperationException($"Trigger '{request.Lease.TriggerId}' not found.");
            }

            var payload = request.Payload ?? string.Empty;
            var metadata = request.Metadata is null
                ? null
                : new Dictionary<string, string>(request.Metadata, StringComparer.OrdinalIgnoreCase);
            var expiresAtUtc = request.Retention > TimeSpan.Zero
                ? request.OccurredAtUtc.Add(request.Retention)
                : (DateTimeOffset?)null;
            var id = Interlocked.Increment(ref _deadLetterSequence);

            entry.DeadLetters.Add(new DeadLetterEntry(
                id,
                request.Lease.TriggerId,
                request.Lease.JobKey,
                request.Lease.Scope,
                request.Lease.FireAtUtc,
                request.Reason,
                payload,
                metadata,
                request.OccurredAtUtc,
                expiresAtUtc));
        }

        return Task.CompletedTask;
    }

    public Task<IReadOnlyCollection<JobDeadLetterEntry>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            var entries = _triggers.Values
                .Where(t => MatchesScope(t.Definition.Scope, scope))
                .SelectMany(t => t.DeadLetters)
                .OrderByDescending(entry => entry.CreatedAtUtc)
                .Select(MapDeadLetter)
                .ToArray();

            return Task.FromResult<IReadOnlyCollection<JobDeadLetterEntry>>(entries);
        }
    }

    public Task<JobDeadLetterEntry?> FindAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            var match = _triggers.Values
                .Where(t => MatchesScope(t.Definition.Scope, scope))
                .SelectMany(t => t.DeadLetters)
                .FirstOrDefault(entry => entry.Id == id);

            return Task.FromResult(match is null ? null : MapDeadLetter(match));
        }
    }

    public Task ResolveAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();

        lock (_lock)
        {
            foreach (var trigger in _triggers.Values)
            {
                if (!MatchesScope(trigger.Definition.Scope, scope))
                {
                    continue;
                }

                var index = trigger.DeadLetters.FindIndex(entry => entry.Id == id);
                if (index >= 0)
                {
                    trigger.DeadLetters.RemoveAt(index);
                    break;
                }
            }
        }

        return Task.CompletedTask;
    }

    private DateTimeOffset UtcNow() => (_options.UtcNowProvider ?? InMemoryJobStoreOptions.DefaultUtcNow)();

    private static bool MatchesScope(PartitionScope a, PartitionScope b)
    {
        return string.Equals(a.TenantId, b.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(a.EnvironmentTag, b.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }

    private static bool JobMatchesScope(string jobKey, PartitionScope scope)
    {
        if (!JobKey.TryParse(jobKey, out var key))
        {
            return false;
        }

        return string.Equals(key.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(key.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }

    private static JobDefinition CloneJob(JobDefinition job)
    {
        IReadOnlyDictionary<string, string>? metadata = job.Metadata is null
            ? null
            : new Dictionary<string, string>(job.Metadata, StringComparer.OrdinalIgnoreCase);

        return new JobDefinition(job.JobKey, job.Namespace, job.Name, job.Variant, job.Description, metadata);
    }

    private static DateTimeOffset? ComputeNextFire(TriggerDefinition trigger, CronSchedule? schedule, DateTimeOffset referenceUtc)
    {
        if (TriggerSchedule.IsOnceExpression(trigger.ScheduleExpression))
        {
            return TriggerSchedule.GetNextOccurrence(trigger.ScheduleExpression, referenceUtc, trigger.StartAtUtc, trigger.EndAtUtc);
        }

        schedule ??= new CronSchedule(trigger.ScheduleExpression);
        var cursor = referenceUtc;

        if (trigger.StartAtUtc.HasValue && trigger.StartAtUtc.Value > cursor)
        {
            cursor = trigger.StartAtUtc.Value;
        }

        var next = schedule.GetNextOccurrence(cursor);
        if (next.HasValue && trigger.EndAtUtc.HasValue && next.Value > trigger.EndAtUtc.Value)
        {
            return null;
        }

        return next;
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

    private string? SerializeMetadata(IReadOnlyDictionary<string, string>? metadata)
    {
        if (metadata is null || metadata.Count == 0) return null;
        return JsonSerializer.Serialize(metadata, _jsonOptions);
    }

    private static JobDeadLetterEntry MapDeadLetter(DeadLetterEntry entry)
    {
        IReadOnlyDictionary<string, string>? metadata = entry.Metadata is null
            ? null
            : new Dictionary<string, string>(entry.Metadata, StringComparer.OrdinalIgnoreCase);

        return new JobDeadLetterEntry(
            entry.Id,
            entry.TriggerId,
            entry.JobKey,
            entry.Scope.TenantId,
            entry.Scope.EnvironmentTag,
            entry.FireAtUtc,
            entry.Reason,
            entry.Payload,
            metadata,
            entry.CreatedAtUtc,
            entry.ExpiresAtUtc);
    }

    private sealed class TriggerEntry
    {
        public TriggerEntry(TriggerDefinition definition, CronSchedule? schedule, DateTimeOffset? nextFireAtUtc)
        {
            Definition = definition;
            Schedule = schedule;
            NextFireAtUtc = nextFireAtUtc;
        }

        public TriggerDefinition Definition { get; }
        public CronSchedule? Schedule { get; }
        public DateTimeOffset? NextFireAtUtc { get; set; }
        public LeaseInfo? Lease { get; set; }
        public List<DeadLetterEntry> DeadLetters { get; } = new();
    }

    private sealed record LeaseInfo(string LeaseId, string InstanceId, DateTimeOffset ExpiresAtUtc, DateTimeOffset FireAtUtc);

    private sealed record DeadLetterEntry(
        long Id,
        string TriggerId,
        string JobKey,
        PartitionScope Scope,
        DateTimeOffset FireAtUtc,
        string Reason,
        string Payload,
        IReadOnlyDictionary<string, string>? Metadata,
        DateTimeOffset CreatedAtUtc,
        DateTimeOffset? ExpiresAtUtc);
}
