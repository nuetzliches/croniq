using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Options;

namespace Croniq.JobStore.InMemory;

public sealed class InMemoryWorkerStore : IWorkerStore
{
    private readonly object _lock = new();
    private readonly Dictionary<(string TenantId, string EnvironmentTag, string InstanceId), WorkerEntry> _entries = new();
    private readonly WorkerStoreOptions _options;

    private sealed record WorkerEntry(
        string InstanceId,
        DateTimeOffset LastSeenAtUtc,
        DateTimeOffset ExpiresAtUtc,
        string? MetadataJson);

    public InMemoryWorkerStore(IOptions<WorkerStoreOptions> options)
    {
        _options = options?.Value ?? new WorkerStoreOptions();
        _options.Normalize();
    }

    public Task UpsertHeartbeatAsync(WorkerHeartbeat heartbeat, CancellationToken cancellationToken)
    {
        if (heartbeat is null) throw new ArgumentNullException(nameof(heartbeat));
        if (string.IsNullOrWhiteSpace(heartbeat.InstanceId)) throw new ArgumentNullException(nameof(heartbeat.InstanceId));

        cancellationToken.ThrowIfCancellationRequested();

        var scope = heartbeat.Scope;
        var instanceId = heartbeat.InstanceId.Trim();
        var now = heartbeat.SeenAtUtc;
        var expiresAt = now.Add(_options.OnlineTtl);

        lock (_lock)
        {
            PruneExpiredUnsafe(scope, now);
            _entries[(scope.TenantId, scope.EnvironmentTag, instanceId)] = new WorkerEntry(instanceId, now, expiresAt, heartbeat.MetadataJson);
        }

        return Task.CompletedTask;
    }

    public Task<IReadOnlyCollection<WorkerStatus>> ListAsync(WorkerQuery query, CancellationToken cancellationToken)
    {
        if (query is null) throw new ArgumentNullException(nameof(query));

        cancellationToken.ThrowIfCancellationRequested();

        var scope = query.Scope;
        var now = query.NowUtc;

        lock (_lock)
        {
            PruneExpiredUnsafe(scope, now);

            var result = _entries
                .Where(kvp => MatchesScope(kvp.Key, scope))
                .Select(kvp => ToStatus(kvp.Value, now))
                .OrderBy(entry => entry.InstanceId, StringComparer.OrdinalIgnoreCase)
                .ToArray();

            return Task.FromResult<IReadOnlyCollection<WorkerStatus>>(result);
        }
    }

    private static bool MatchesScope((string TenantId, string EnvironmentTag, string InstanceId) key, PartitionScope scope)
    {
        return string.Equals(key.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(key.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }

    private static WorkerStatus ToStatus(WorkerEntry entry, DateTimeOffset now)
    {
        var online = entry.ExpiresAtUtc > now;
        return new WorkerStatus(entry.InstanceId, entry.LastSeenAtUtc, entry.ExpiresAtUtc, online, entry.MetadataJson);
    }

    private void PruneExpiredUnsafe(PartitionScope scope, DateTimeOffset now)
    {
        var expiredKeys = _entries
            .Where(kvp => MatchesScope(kvp.Key, scope) && kvp.Value.ExpiresAtUtc <= now)
            .Select(kvp => kvp.Key)
            .ToList();

        foreach (var key in expiredKeys)
        {
            _entries.Remove(key);
        }
    }
}
