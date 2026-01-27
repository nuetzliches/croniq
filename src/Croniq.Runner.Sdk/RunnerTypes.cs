using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Runner;

public sealed record Lease(
    string ExecutionId,
    string LeaseId,
    string TriggerId,
    string JobKey,
    DateTimeOffset FireAtUtc,
    DateTimeOffset LeaseExpiresAtUtc,
    string? Payload,
    string? ExecutionMode,
    string? InvocationSource);

public sealed record WorkEvent(
    string Message,
    string? Level = null,
    DateTimeOffset? TimestampUtc = null,
    IReadOnlyDictionary<string, string>? Properties = null,
    string? EventType = null);

public sealed record RunnerExecutionContext(
    string ExecutionId,
    string LeaseId,
    string TriggerId,
    string JobKey,
    DateTimeOffset FireAtUtc,
    DateTimeOffset LeaseExpiresAtUtc,
    string? ExecutionMode,
    string? InvocationSource,
    CancellationToken CancellationToken,
    Func<WorkEvent, Task>? EmitEventAsync);

public delegate Task RunnerExecuteHandler(
    RunnerExecutionContext context,
    object? payload,
    IRunnerLogger logger,
    CancellationToken cancellationToken);
