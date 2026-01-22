using System;
using System.Collections.Generic;
using System.ComponentModel.DataAnnotations;

namespace Croniq.Api.Models;

public sealed record UpsertWebhookEndpointRequest(
    [property: Required] string HookKey,
    [property: Required] string JobKey,
    bool Enabled = true,
    bool RequireSignature = true,
    int? RequestsPerMinute = null,
    string? Secret = null,
    IDictionary<string, string>? Metadata = null,
    int SignatureVersion = 1,
    bool AllowUnsigned = false);

public sealed record WebhookEndpointResponse(
    string HookKey,
    string JobKey,
    bool Enabled,
    bool RequireSignature,
    int RequestsPerMinute,
    IDictionary<string, string>? Metadata,
    IReadOnlyCollection<WebhookIpRuleResponse> IpRules,
    string Status,
    DateTimeOffset? LastDeliveryAtUtc,
    int? IpRuleCount,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset UpdatedAtUtc,
    string? Secret = null);

public sealed record WebhookEndpointEventResponse(
    long Id,
    string HookKey,
    string EventType,
    DateTime OccurredAtUtc,
    string? Actor,
    string? CorrelationId);

public sealed record WebhookCapabilitiesResponse(
    bool AllowUnsignedHooks,
    int DefaultRequestsPerMinute,
    string Mode,
    string? RemoteBaseUrl,
    string? RemoteIngressBaseUrl);

public sealed record WebhookRemoteHealthResponse(
    string Status,
    DateTimeOffset CheckedAtUtc,
    int? StatusCode,
    string? Detail);

public sealed record WebhookDeadLetterResponse(
    long Id,
    string HookKey,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    string Payload,
    IDictionary<string, string>? Headers,
    IDictionary<string, string>? Metadata,
    string FailureReason,
    int Attempts,
    int? StatusCode,
    string? ErrorDetails,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset? LastAttemptAtUtc,
    DateTimeOffset? NextAttemptAtUtc,
    DateTimeOffset? ExpiresAtUtc);

public sealed record WebhookReplayResult(
    string Status,
    string Hook,
    string Job);

public sealed record WebhookInvokeResult(
    string Status,
    string Hook,
    string Job,
    string ExecutionId);

public sealed record RotateWebhookSecretRequest(
    int? ActivateInSeconds = null,
    int? GracePeriodSeconds = null,
    string? Notes = null);

public sealed record RotateWebhookSecretResponse(
    string HookKey,
    DateTime ActivatedAtUtc,
    DateTime? ExpiresAtUtc,
    string Secret,
    string SecretHash);

public sealed record CreateWebhookIpRuleRequest(
    [property: Required] string Cidr,
    string? Description = null);

public sealed record WebhookIpRuleResponse(
    long Id,
    string Cidr,
    string? Description,
    string? CreatedBy,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset UpdatedAtUtc);

public sealed record WebhookDeadLetterFailureRequest(
    [property: Required] string FailureReason,
    int? StatusCode = null,
    string? ErrorDetails = null,
    DateTimeOffset? NextAttemptAtUtc = null);
