using System;

namespace Croniq.Persistence.Abstractions;

public sealed record WorkAssignment(
    PartitionScope Scope,
    string ExecutionId,
    string JobKey,
    string? TriggerId,
    int Attempt,
    string RunnerId,
    string LeaseId,
    DateTimeOffset LeaseExpiresAtUtc,
    string? Payload,
    DateTimeOffset AssignedAtUtc);
