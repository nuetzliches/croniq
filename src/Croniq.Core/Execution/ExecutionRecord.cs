using System;

namespace Croniq.Core.Execution;

/// <summary>
/// Snapshot captured when an execution starts. Storage implementations can persist it alongside log entries.
/// </summary>
public sealed record ExecutionRecord(
    string ExecutionId,
    ExecutionKind Kind,
    string? WorkflowId,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    string? TriggerId,
    DateTimeOffset FireAtUtc,
    DateTimeOffset StartedAtUtc,
    string? InstanceId,
    string? TraceId,
    string? SpanId,
    string? CorrelationId,
    string ExecutionMode = Persistence.Abstractions.ExecutionIntent.ExecutionModes.Normal,
    string InvocationSource = Persistence.Abstractions.ExecutionIntent.InvocationSources.Schedule);
