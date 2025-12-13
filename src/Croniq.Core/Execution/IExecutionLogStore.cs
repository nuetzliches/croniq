using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Core.Execution;

/// <summary>
/// Abstraction for persisting execution metadata and log entries (jobs, workflows, etc.).
/// </summary>
public interface IExecutionLogStore
{
    Task OnExecutionStartedAsync(ExecutionRecord record, CancellationToken cancellationToken);

    Task AppendAsync(string executionId, IReadOnlyCollection<ExecutionLogEntry> entries, CancellationToken cancellationToken);

    Task OnExecutionCompletedAsync(ExecutionCompletion completion, CancellationToken cancellationToken);
}
