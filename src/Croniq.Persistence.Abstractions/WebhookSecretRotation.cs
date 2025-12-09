using System;

namespace Croniq.Persistence.Abstractions;

public sealed record WebhookSecretRotate(
    string HookKey,
    string TenantId,
    string EnvironmentTag,
    int? ActivateInSeconds,
    int? GracePeriodSeconds,
    string? RotatedBy,
    string? Notes);

public sealed record WebhookSecretRotationResult(
    string HookKey,
    string Secret,
    string SecretHash,
    DateTime ActivatedAtUtc,
    DateTime? ExpiresAtUtc);

public sealed record WebhookSecretMaterial(
    string Secret,
    string SecretHash,
    DateTime ActivatedAtUtc,
    DateTime? ExpiresAtUtc);
