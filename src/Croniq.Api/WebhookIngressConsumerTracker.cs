using System;
using System.Collections.Concurrent;
using Croniq.Persistence.Abstractions;

namespace Croniq.Api;

internal sealed class WebhookIngressConsumerTracker
{
    private readonly ConcurrentDictionary<string, ConcurrentDictionary<string, LeaseEntry>> _consumers =
        new(StringComparer.OrdinalIgnoreCase);

    public void Reset(string consumerId)
    {
        if (!TryNormalize(consumerId, out var normalized))
        {
            return;
        }

        _consumers[normalized] = new ConcurrentDictionary<string, LeaseEntry>(StringComparer.OrdinalIgnoreCase);
    }

    public void RemoveConsumer(string consumerId)
    {
        if (!TryNormalize(consumerId, out var normalized))
        {
            return;
        }

        _consumers.TryRemove(normalized, out _);
    }

    public int GetCount(string consumerId)
    {
        if (!TryNormalize(consumerId, out var normalized))
        {
            return 0;
        }

        return _consumers.TryGetValue(normalized, out var leases)
            ? leases.Count
            : 0;
    }

    public void RemoveExpired(string consumerId, DateTimeOffset now)
    {
        if (!TryNormalize(consumerId, out var normalized))
        {
            return;
        }

        if (!_consumers.TryGetValue(normalized, out var leases))
        {
            return;
        }

        foreach (var pair in leases)
        {
            if (pair.Value.ExpiresAtUtc <= now)
            {
                leases.TryRemove(pair.Key, out _);
            }
        }
    }

    public void AddLease(string consumerId, WebhookIngressLease lease)
    {
        if (lease is null || string.IsNullOrWhiteSpace(lease.LeaseId))
        {
            return;
        }

        if (!TryNormalize(consumerId, out var normalized))
        {
            return;
        }

        var leases = _consumers.GetOrAdd(normalized,
            _ => new ConcurrentDictionary<string, LeaseEntry>(StringComparer.OrdinalIgnoreCase));
        leases[lease.LeaseId] = new LeaseEntry(lease.EventId, lease.LeaseExpiresAtUtc);
    }

    public void RemoveLease(string? consumerId, string leaseId)
    {
        if (!TryNormalize(consumerId, out var normalized))
        {
            return;
        }

        if (!_consumers.TryGetValue(normalized, out var leases))
        {
            return;
        }

        leases.TryRemove(leaseId, out _);
    }

    public void UpdateLeaseExpiry(string? consumerId, string leaseId, DateTimeOffset newExpiry)
    {
        if (!TryNormalize(consumerId, out var normalized))
        {
            return;
        }

        if (!_consumers.TryGetValue(normalized, out var leases))
        {
            return;
        }

        leases.AddOrUpdate(
            leaseId,
            _ => new LeaseEntry(string.Empty, newExpiry),
            (_, existing) => existing with { ExpiresAtUtc = newExpiry });
    }

    private static bool TryNormalize(string? consumerId, out string normalized)
    {
        normalized = string.Empty;
        if (string.IsNullOrWhiteSpace(consumerId))
        {
            return false;
        }

        normalized = consumerId.Trim();
        return normalized.Length > 0;
    }

    internal readonly record struct LeaseEntry(string EventId, DateTimeOffset ExpiresAtUtc);
}
