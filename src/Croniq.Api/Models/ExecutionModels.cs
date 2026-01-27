using System;
using Croniq.Core.Execution;

namespace Croniq.Api.Models;

public sealed record ExecutionResponse(
    string ExecutionId,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    ExecutionKind Kind,
    ExecutionStatus? Status,
    DateTimeOffset FireAtUtc,
    DateTimeOffset StartedAtUtc,
    DateTimeOffset? CompletedAtUtc,
    double? DurationMs,
    string? TriggerId,
    string? InstanceId,
    string? TraceId,
    string? CorrelationId,
    string? ErrorType,
    string? ErrorMessage,
    string ExecutionMode = Persistence.Abstractions.ExecutionIntent.ExecutionModes.Normal,
    string InvocationSource = Persistence.Abstractions.ExecutionIntent.InvocationSources.Schedule);
