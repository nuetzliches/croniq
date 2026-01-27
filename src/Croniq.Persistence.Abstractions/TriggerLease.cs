using System;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Represents a leased trigger ready for execution.
/// </summary>
public sealed record TriggerLease(
    string LeaseId,
    string TriggerId,
    string JobKey,
    PartitionScope Scope,
    DateTimeOffset FireAtUtc,
    DateTimeOffset LeaseExpiresAtUtc,
    string? Payload,
    string? ExecutionId = null,
    string ExecutionMode = ExecutionIntent.ExecutionModes.Normal,
    string InvocationSource = ExecutionIntent.InvocationSources.Schedule);
