using System;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Changefeed entry representing a webhook endpoint mutation.
/// </summary>
public sealed record WebhookEndpointEvent(
    long Id,
    string HookKey,
    string TenantId,
    string EnvironmentTag,
    string EventType,
    DateTime OccurredAtUtc,
    string? Actor,
    string? CorrelationId);
