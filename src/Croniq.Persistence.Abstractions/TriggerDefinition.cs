using System;
using System.Collections.Generic;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Trigger definition as persisted in a store.
/// </summary>
public sealed record TriggerDefinition(
    string TriggerId,
    string JobKey,
    string ScheduleExpression,
    PartitionScope Scope,
    DateTimeOffset? StartAtUtc = null,
    DateTimeOffset? EndAtUtc = null,
    bool Enabled = true,
    IReadOnlyDictionary<string, string>? Metadata = null,
    string? TimeZoneId = null);
