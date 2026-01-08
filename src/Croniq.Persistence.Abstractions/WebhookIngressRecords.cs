using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

public interface IWebhookIngressEventStore
{
    Task EnqueueAsync(WebhookIngressEventCreate request, CancellationToken cancellationToken);

    Task<IReadOnlyCollection<WebhookIngressLease>> AcquireAsync(WebhookIngressAcquireRequest request, CancellationToken cancellationToken);

    Task<bool> TryExtendLeaseAsync(WebhookIngressLeaseRenewal renewal, CancellationToken cancellationToken);

    Task AcknowledgeAsync(WebhookIngressAck ack, CancellationToken cancellationToken);

    Task NackAsync(WebhookIngressNack nack, CancellationToken cancellationToken);
}

public sealed record WebhookIngressEventCreate(
    string EventId,
    string HookKey,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    string Payload,
    IReadOnlyDictionary<string, string>? Headers,
    IReadOnlyDictionary<string, string>? Metadata,
    DateTimeOffset ReceivedAtUtc);

public sealed record WebhookIngressAcquireRequest(
    PartitionScope Scope,
    DateTimeOffset NowUtc,
    int MaxCount,
    TimeSpan LeaseDuration);

public sealed record WebhookIngressLease(
    string EventId,
    string LeaseId,
    DateTimeOffset LeaseExpiresAtUtc,
    string HookKey,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    string Payload,
    IReadOnlyDictionary<string, string>? Headers,
    IReadOnlyDictionary<string, string>? Metadata,
    DateTimeOffset ReceivedAtUtc);

public sealed record WebhookIngressLeaseRenewal(
    string EventId,
    string LeaseId,
    DateTimeOffset LeaseExpiresAtUtc,
    DateTimeOffset RenewedAtUtc);

public sealed record WebhookIngressAck(
    string EventId,
    string LeaseId,
    bool Succeeded,
    string? ErrorMessage,
    DateTimeOffset AcknowledgedAtUtc);

public sealed record WebhookIngressNack(
    string EventId,
    string LeaseId,
    string? Reason,
    DateTimeOffset NackedAtUtc);
