using System.Collections.Generic;
using System.ComponentModel.DataAnnotations;

namespace Croniq.Api.Models;

public sealed record WebhookIngressEventToken(
    [property: Required] string EventId,
    [property: Required] string LeaseId,
    long LeaseExpiresAtUtc,
    [property: Required] string HookKey,
    [property: Required] string JobKey,
    string Payload,
    IDictionary<string, string>? Headers,
    long ReceivedAtUtc,
    IDictionary<string, string>? Metadata);

public sealed record WebhookIngressPollResponse(
    WebhookIngressEventToken[] Events,
    long ServerTimeUtc);

public sealed record WebhookIngressAckRequest(
    [property: Required] string EventId,
    [property: Required] string LeaseId,
    bool Succeeded,
    string? ErrorMessage = null,
    string? ConsumerId = null);

public sealed record WebhookIngressNackRequest(
    [property: Required] string EventId,
    [property: Required] string LeaseId,
    string? Reason = null,
    string? ConsumerId = null);

public sealed record WebhookIngressExtendRequest(
    [property: Required] string EventId,
    [property: Required] string LeaseId,
    [property: Required] long LeaseExpiresAtUtc,
    string? ConsumerId = null);

public sealed record WebhookIngressExtendResponse(
    bool Extended);
