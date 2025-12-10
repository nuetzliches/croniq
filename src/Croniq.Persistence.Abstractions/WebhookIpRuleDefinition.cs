using System;

namespace Croniq.Persistence.Abstractions;

public sealed record WebhookIpRuleDefinition(
    long Id,
    string HookKey,
    string TenantId,
    string EnvironmentTag,
    string Cidr,
    string? Description,
    string? CreatedBy,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset UpdatedAtUtc);

public sealed record WebhookIpRuleCreate(
    string HookKey,
    string TenantId,
    string EnvironmentTag,
    string Cidr,
    string? Description,
    string? CreatedBy);
