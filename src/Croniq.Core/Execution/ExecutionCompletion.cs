using System;

namespace Croniq.Core.Execution;

/// <summary>
/// Captures the terminal state of an execution for persistence.
/// </summary>
public sealed record ExecutionCompletion(
    string ExecutionId,
    DateTimeOffset CompletedAtUtc,
    ExecutionStatus Status,
    double? DurationMs = null,
    string? ErrorType = null,
    string? ErrorMessage = null);
