using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Core.Execution;

/// <summary>
/// Default execution log store that intentionally does nothing. Used when persistence is disabled.
/// </summary>
public sealed class NoOpExecutionLogStore : IExecutionLogStore
{
    public Task OnExecutionStartedAsync(ExecutionRecord record, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task AppendAsync(string executionId, IReadOnlyCollection<ExecutionLogEntry> entries, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task OnExecutionCompletedAsync(ExecutionCompletion completion, CancellationToken cancellationToken) => Task.CompletedTask;
}
