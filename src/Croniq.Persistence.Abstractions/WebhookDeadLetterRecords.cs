using System;
using System.Collections.Generic;

namespace Croniq.Persistence.Abstractions;

public sealed record WebhookDeadLetterEntry(
    long Id,
    string HookKey,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    string Payload,
    IReadOnlyDictionary<string, string>? Headers,
    IReadOnlyDictionary<string, string>? Metadata,
    string FailureReason,
    int Attempts,
    int? StatusCode,
    string? ErrorDetails,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset? LastAttemptAtUtc,
    DateTimeOffset? NextAttemptAtUtc,
    DateTimeOffset? ExpiresAtUtc);

public sealed record WebhookDeadLetterCreate(
    string HookKey,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    string Payload,
    IReadOnlyDictionary<string, string>? Headers,
    IReadOnlyDictionary<string, string>? Metadata,
    string FailureReason,
    int? StatusCode,
    string? ErrorDetails,
    DateTimeOffset? ExpiresAtUtc);

public sealed record WebhookDeadLetterFailure(
    string FailureReason,
    int? StatusCode,
    string? ErrorDetails,
    DateTimeOffset? NextAttemptAtUtc);
