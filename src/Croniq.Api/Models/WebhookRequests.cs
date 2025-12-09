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
    int SignatureVersion = 1);

public sealed record WebhookEndpointResponse(
    string HookKey,
    string JobKey,
    bool Enabled,
    bool RequireSignature,
    int RequestsPerMinute,
    IDictionary<string, string>? Metadata,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset UpdatedAtUtc,
    string? Secret = null);

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
