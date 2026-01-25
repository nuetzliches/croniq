using System;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;
using Croniq.Core.Jobs;

namespace Croniq.Core.Execution;

/// <summary>
/// Dispatch sink that persists or forwards worker execution lifecycle events.
/// </summary>
public interface ITriggerWorkerDispatchSink
{
    Task OnAssignmentAsync(TriggerLease lease, CancellationToken cancellationToken);

    Task OnExecutionStartedAsync(
        string executionId,
        TriggerLease lease,
        JobKey jobKey,
        Activity? activity,
        CancellationToken cancellationToken);

    Task OnExecutionCompletedAsync(
        string executionId,
        ExecutionStatus status,
        double? durationMs,
        Exception? error,
        CancellationToken cancellationToken);

    Task OnReleaseAsync(TriggerReleaseRequest release, CancellationToken cancellationToken);
}