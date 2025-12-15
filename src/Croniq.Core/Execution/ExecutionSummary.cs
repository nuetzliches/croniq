using System;

namespace Croniq.Core.Execution;

/// <summary>
/// Snapshot describing a single execution for list/detail views.
/// </summary>
public sealed record ExecutionSummary(
    string ExecutionId,
    ExecutionKind Kind,
    string? WorkflowId,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    string? TriggerId,
    DateTimeOffset FireAtUtc,
    DateTimeOffset StartedAtUtc,
    DateTimeOffset? CompletedAtUtc,
    ExecutionStatus? Status,
    double? DurationMs,
    string? InstanceId,
    string? TraceId,
    string? CorrelationId,
    string? ErrorType,
    string? ErrorMessage);
