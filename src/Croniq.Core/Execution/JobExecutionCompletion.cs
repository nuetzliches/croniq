using System;

namespace Croniq.Core.Execution;

/// <summary>
/// Captures the terminal state of a job execution for persistence.
/// </summary>
public sealed record JobExecutionCompletion(
    string ExecutionId,
    DateTimeOffset CompletedAtUtc,
    JobExecutionStatus Status,
    double? DurationMs = null,
    string? ErrorType = null,
    string? ErrorMessage = null);
