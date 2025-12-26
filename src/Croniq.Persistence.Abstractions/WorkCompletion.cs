using System;

namespace Croniq.Persistence.Abstractions;

public sealed record WorkCompletion(
    string LeaseId,
    string RunnerId,
    bool Succeeded,
    DateTimeOffset CompletedAtUtc,
    string? DeadLetterReason = null,
    string? ExecutionId = null);
