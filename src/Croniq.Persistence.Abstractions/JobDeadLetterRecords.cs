using System;
using System.Collections.Generic;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Represents a persisted dead-letter entry for a scheduled trigger.
/// </summary>
public sealed record JobDeadLetterEntry(
    long Id,
    string TriggerId,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    DateTimeOffset FireAtUtc,
    string Reason,
    string Payload,
    IReadOnlyDictionary<string, string>? Metadata,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset? ExpiresAtUtc);
