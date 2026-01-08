using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;

namespace Croniq.Api.Tests.Infrastructure;

public sealed class InMemoryWebhookIngressEventStore : IWebhookIngressEventStore
{
    public const string StatusPending = "Pending";
    public const string StatusLeased = "Leased";
    public const string StatusDelivered = "Delivered";
    public const string StatusFailed = "Failed";

    private readonly object _sync = new();
    private readonly Dictionary<string, Entry> _entries = new(StringComparer.OrdinalIgnoreCase);

    public TimeSpan? LeaseDurationOverride { get; set; }

    public Task EnqueueAsync(WebhookIngressEventCreate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (string.IsNullOrWhiteSpace(request.EventId)) throw new ArgumentNullException(nameof(request.EventId));

        lock (_sync)
        {
            if (_entries.ContainsKey(request.EventId))
            {
                return Task.CompletedTask;
            }

            _entries[request.EventId] = new Entry(
                request,
                StatusPending,
                null,
                null,
                0,
                null);
        }

        return Task.CompletedTask;
    }

    public Task<IReadOnlyCollection<WebhookIngressLease>> AcquireAsync(
        WebhookIngressAcquireRequest request,
        CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        if (request.MaxCount <= 0)
        {
            return Task.FromResult<IReadOnlyCollection<WebhookIngressLease>>(Array.Empty<WebhookIngressLease>());
        }

        if (request.LeaseDuration <= TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(nameof(request.LeaseDuration));
        }

        List<WebhookIngressLease> leases = new();

        lock (_sync)
        {
            var now = request.NowUtc;
            var leaseDuration = LeaseDurationOverride ?? request.LeaseDuration;
            var candidates = _entries.Values
                .Where(entry => string.Equals(entry.Request.TenantId, request.Scope.TenantId, StringComparison.OrdinalIgnoreCase)
                    && string.Equals(entry.Request.EnvironmentTag, request.Scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
                .Where(entry => entry.Status == StatusPending
                    || (entry.Status == StatusLeased && entry.LeaseExpiresAtUtc is not null && entry.LeaseExpiresAtUtc <= now))
                .OrderBy(entry => entry.Request.ReceivedAtUtc)
                .ThenBy(entry => entry.Request.EventId, StringComparer.OrdinalIgnoreCase)
                .Take(request.MaxCount)
                .ToList();

            foreach (var entry in candidates)
            {
                var leaseId = Guid.NewGuid().ToString("N");
                var leaseExpiresAtUtc = now.Add(leaseDuration);
                var updated = entry with
                {
                    Status = StatusLeased,
                    LeaseId = leaseId,
                    LeaseExpiresAtUtc = leaseExpiresAtUtc,
                    AttemptCount = entry.AttemptCount + 1
                };

                _entries[entry.Request.EventId] = updated;
                leases.Add(new WebhookIngressLease(
                    entry.Request.EventId,
                    leaseId,
                    leaseExpiresAtUtc,
                    entry.Request.HookKey,
                    entry.Request.JobKey,
                    entry.Request.TenantId,
                    entry.Request.EnvironmentTag,
                    entry.Request.Payload,
                    entry.Request.Headers,
                    entry.Request.Metadata,
                    entry.Request.ReceivedAtUtc));
            }
        }

        return Task.FromResult<IReadOnlyCollection<WebhookIngressLease>>(leases);
    }

    public Task<bool> TryExtendLeaseAsync(WebhookIngressLeaseRenewal renewal, CancellationToken cancellationToken)
    {
        if (renewal is null) throw new ArgumentNullException(nameof(renewal));

        lock (_sync)
        {
            if (!_entries.TryGetValue(renewal.EventId, out var entry))
            {
                return Task.FromResult(false);
            }

            if (!string.Equals(entry.LeaseId, renewal.LeaseId, StringComparison.OrdinalIgnoreCase))
            {
                return Task.FromResult(false);
            }

            if (entry.LeaseExpiresAtUtc is null || entry.LeaseExpiresAtUtc <= renewal.RenewedAtUtc)
            {
                return Task.FromResult(false);
            }

            var updated = entry with { LeaseExpiresAtUtc = renewal.LeaseExpiresAtUtc };
            _entries[entry.Request.EventId] = updated;
            return Task.FromResult(true);
        }
    }

    public Task AcknowledgeAsync(WebhookIngressAck ack, CancellationToken cancellationToken)
    {
        if (ack is null) throw new ArgumentNullException(nameof(ack));

        lock (_sync)
        {
            if (!_entries.TryGetValue(ack.EventId, out var entry))
            {
                return Task.CompletedTask;
            }

            if (!string.Equals(entry.LeaseId, ack.LeaseId, StringComparison.OrdinalIgnoreCase))
            {
                return Task.CompletedTask;
            }

            var updated = entry with
            {
                Status = ack.Succeeded ? StatusDelivered : StatusFailed,
                LeaseId = null,
                LeaseExpiresAtUtc = null,
                LastError = ack.Succeeded ? null : ack.ErrorMessage
            };

            _entries[entry.Request.EventId] = updated;
        }

        return Task.CompletedTask;
    }

    public Task NackAsync(WebhookIngressNack nack, CancellationToken cancellationToken)
    {
        if (nack is null) throw new ArgumentNullException(nameof(nack));

        lock (_sync)
        {
            if (!_entries.TryGetValue(nack.EventId, out var entry))
            {
                return Task.CompletedTask;
            }

            if (!string.Equals(entry.LeaseId, nack.LeaseId, StringComparison.OrdinalIgnoreCase))
            {
                return Task.CompletedTask;
            }

            var updated = entry with
            {
                Status = StatusPending,
                LeaseId = null,
                LeaseExpiresAtUtc = null,
                LastError = nack.Reason
            };

            _entries[entry.Request.EventId] = updated;
        }

        return Task.CompletedTask;
    }

    public IngressEventSnapshot? GetSnapshot(string eventId)
    {
        if (string.IsNullOrWhiteSpace(eventId)) throw new ArgumentNullException(nameof(eventId));

        lock (_sync)
        {
            if (!_entries.TryGetValue(eventId, out var entry))
            {
                return null;
            }

            return new IngressEventSnapshot(
                entry.Request.EventId,
                entry.Status,
                entry.LeaseId,
                entry.LeaseExpiresAtUtc,
                entry.AttemptCount,
                entry.LastError);
        }
    }

    public void Clear()
    {
        lock (_sync)
        {
            _entries.Clear();
        }
    }

    private sealed record Entry(
        WebhookIngressEventCreate Request,
        string Status,
        string? LeaseId,
        DateTimeOffset? LeaseExpiresAtUtc,
        int AttemptCount,
        string? LastError);

    public sealed record IngressEventSnapshot(
        string EventId,
        string Status,
        string? LeaseId,
        DateTimeOffset? LeaseExpiresAtUtc,
        int AttemptCount,
        string? LastError);
}
