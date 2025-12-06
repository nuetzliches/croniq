using System;
using System.Collections.Generic;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Encapsulates the data required to persist a failed trigger execution into a dead-letter store.
/// </summary>
public sealed record DeadLetterRequest(
    TriggerLease Lease,
    string Reason,
    DateTimeOffset OccurredAtUtc,
    TimeSpan Retention,
    string? Payload = null,
    IReadOnlyDictionary<string, string>? Metadata = null);
