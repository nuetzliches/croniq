using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Options;

namespace Croniq.JobStore.InMemory;

public sealed class InMemoryRunnerStore : IRunnerStore
{
    private readonly object _lock = new();
    private readonly Dictionary<(string TenantId, string EnvironmentTag, string RunnerId), RunnerEntry> _entries = new();
    private readonly RunnerStoreOptions _options;

    private sealed record RunnerEntry(
        string RunnerId,
        DateTimeOffset LastSeenAtUtc,
        DateTimeOffset ExpiresAtUtc,
        string? MetadataJson);

    public InMemoryRunnerStore(IOptions<RunnerStoreOptions> options)
    {
        _options = options?.Value ?? new RunnerStoreOptions();
        _options.Normalize();
    }

    public Task UpsertHeartbeatAsync(RunnerHeartbeat heartbeat, CancellationToken cancellationToken)
    {
        if (heartbeat is null) throw new ArgumentNullException(nameof(heartbeat));
        if (string.IsNullOrWhiteSpace(heartbeat.RunnerId)) throw new ArgumentNullException(nameof(heartbeat.RunnerId));

        cancellationToken.ThrowIfCancellationRequested();

        var scope = heartbeat.Scope;
        var runnerId = heartbeat.RunnerId.Trim();
        var now = heartbeat.SeenAtUtc;
        var expiresAt = now.Add(_options.OnlineTtl);

        lock (_lock)
        {
            PruneExpiredUnsafe(scope, now);
            _entries[(scope.TenantId, scope.EnvironmentTag, runnerId)] = new RunnerEntry(runnerId, now, expiresAt, heartbeat.MetadataJson);
        }

        return Task.CompletedTask;
    }

    public Task<IReadOnlyCollection<RunnerStatus>> ListAsync(RunnerQuery query, CancellationToken cancellationToken)
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
                .OrderBy(r => r.RunnerId, StringComparer.OrdinalIgnoreCase)
                .ToArray();

            return Task.FromResult<IReadOnlyCollection<RunnerStatus>>(result);
        }
    }

    public Task<RunnerStatus?> TryGetAsync(RunnerLookup lookup, CancellationToken cancellationToken)
    {
        if (lookup is null) throw new ArgumentNullException(nameof(lookup));

        cancellationToken.ThrowIfCancellationRequested();

        var scope = lookup.Scope;
        var now = lookup.NowUtc;
        var runnerId = lookup.RunnerId?.Trim();
        if (string.IsNullOrWhiteSpace(runnerId))
        {
            return Task.FromResult<RunnerStatus?>(null);
        }

        lock (_lock)
        {
            PruneExpiredUnsafe(scope, now);

            if (_entries.TryGetValue((scope.TenantId, scope.EnvironmentTag, runnerId), out var entry))
            {
                return Task.FromResult<RunnerStatus?>(ToStatus(entry, now));
            }
        }

        return Task.FromResult<RunnerStatus?>(null);
    }

    private static bool MatchesScope((string TenantId, string EnvironmentTag, string RunnerId) key, PartitionScope scope)
    {
        return string.Equals(key.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(key.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }

    private static RunnerStatus ToStatus(RunnerEntry entry, DateTimeOffset now)
    {
        var online = entry.ExpiresAtUtc > now;
        return new RunnerStatus(entry.RunnerId, entry.LastSeenAtUtc, entry.ExpiresAtUtc, online, entry.MetadataJson);
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
