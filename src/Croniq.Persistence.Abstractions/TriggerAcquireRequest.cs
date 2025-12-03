using System;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Request to acquire due triggers for execution with pessimistic locking.
/// </summary>
public sealed record TriggerAcquireRequest(
    PartitionScope Scope,
    string InstanceId,
    DateTimeOffset NowUtc,
    int BatchSize);
