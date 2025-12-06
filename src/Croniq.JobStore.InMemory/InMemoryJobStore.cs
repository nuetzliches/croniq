using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Options;

namespace Croniq.JobStore.InMemory;

/// <summary>
/// Reference in-memory implementation of the persistence abstractions.
/// </summary>
public sealed class InMemoryJobStore : IJobPersistenceProvider
{
    private readonly object _lock = new();
    private readonly Dictionary<string, JobDefinition> _jobs = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, TriggerEntry> _triggers = new(StringComparer.OrdinalIgnoreCase);
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);
    private readonly InMemoryJobStoreOptions _options;
    private long _leaseSequence;

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
            _jobs[job.JobKey] = job;
        }

        return Task.CompletedTask;
    }

    public Task UpsertTriggerAsync(TriggerDefinition trigger, CancellationToken cancellationToken)
    {
        if (trigger is null) throw new ArgumentNullException(nameof(trigger));
        cancellationToken.ThrowIfCancellationRequested();

        var schedule = new CronSchedule(trigger.ScheduleExpression);
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
                entry.DeadLetters.Add(new DeadLetterEntry(request.DeadLetterReason!, UtcNow()));
            }

            entry.Lease = null;

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

            entry.DeadLetters.Add(new DeadLetterEntry(
                request.Reason,
                request.Payload,
                request.Metadata is null ? null : new Dictionary<string, string>(request.Metadata, StringComparer.OrdinalIgnoreCase),
                request.OccurredAtUtc,
                request.Retention));
        }

        return Task.CompletedTask;
    }

    private DateTimeOffset UtcNow() => (_options.UtcNowProvider ?? InMemoryJobStoreOptions.DefaultUtcNow)();

    private static bool MatchesScope(PartitionScope a, PartitionScope b)
    {
        return string.Equals(a.TenantId, b.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(a.EnvironmentTag, b.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }

    private static DateTimeOffset? ComputeNextFire(TriggerDefinition trigger, CronSchedule schedule, DateTimeOffset referenceUtc)
    {
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

    private string? SerializeMetadata(IReadOnlyDictionary<string, string>? metadata)
    {
        if (metadata is null || metadata.Count == 0) return null;
        return JsonSerializer.Serialize(metadata, _jsonOptions);
    }

    private sealed class TriggerEntry
    {
        public TriggerEntry(TriggerDefinition definition, CronSchedule schedule, DateTimeOffset? nextFireAtUtc)
        {
            Definition = definition;
            Schedule = schedule;
            NextFireAtUtc = nextFireAtUtc;
        }

        public TriggerDefinition Definition { get; }
        public CronSchedule Schedule { get; }
        public DateTimeOffset? NextFireAtUtc { get; set; }
        public LeaseInfo? Lease { get; set; }
        public List<DeadLetterEntry> DeadLetters { get; } = new();
    }

    private sealed record LeaseInfo(string LeaseId, string InstanceId, DateTimeOffset ExpiresAtUtc, DateTimeOffset FireAtUtc);

    private sealed record DeadLetterEntry(
        string Reason,
        string? Payload,
        IReadOnlyDictionary<string, string>? Metadata,
        DateTimeOffset CreatedAtUtc,
        TimeSpan Retention);
}
