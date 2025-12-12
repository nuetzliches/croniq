using System;

namespace Croniq.Core.Execution;

/// <summary>
/// Snapshot captured when a job execution starts. Storage implementations can persist it alongside log entries.
/// </summary>
public sealed record JobExecutionRecord(
    string ExecutionId,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    string? TriggerId,
    DateTimeOffset FireAtUtc,
    DateTimeOffset StartedAtUtc,
    string? InstanceId,
    string? TraceId,
    string? SpanId,
    string? CorrelationId);
