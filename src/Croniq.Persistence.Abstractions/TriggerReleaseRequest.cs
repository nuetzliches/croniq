using System;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Releases a leased trigger and optionally schedules its next fire time or dead-letters it.
/// </summary>
public sealed record TriggerReleaseRequest(
    TriggerLease Lease,
    bool Succeeded,
    DateTimeOffset? NextFireTimeUtc,
    string? DeadLetterReason = null);
