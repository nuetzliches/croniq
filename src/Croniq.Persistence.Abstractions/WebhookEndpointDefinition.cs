using System;
using System.Collections.Generic;

namespace Croniq.Persistence.Abstractions;

public sealed record WebhookEndpointDefinition(
    string HookKey,
    string JobKey,
    string Secret,
    bool Enabled,
    bool RequireSignature,
    int RequestsPerMinute,
    string TenantId,
    string EnvironmentTag,
    IReadOnlyDictionary<string, string>? Metadata,
    int SignatureVersion,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset UpdatedAtUtc);

public sealed record WebhookEndpointUpsert(
    string HookKey,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    bool Enabled,
    bool RequireSignature,
    int RequestsPerMinute,
    string? Secret,
    int SignatureVersion,
    IReadOnlyDictionary<string, string>? Metadata);
