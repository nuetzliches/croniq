using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Core.Execution;

/// <summary>
/// Default log store that intentionally does nothing. Used when persistence is disabled.
/// </summary>
public sealed class NoOpJobLogStore : IJobLogStore
{
    public Task OnExecutionStartedAsync(JobExecutionRecord record, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task AppendAsync(string executionId, IReadOnlyCollection<JobLogEntry> entries, CancellationToken cancellationToken) => Task.CompletedTask;

    public Task OnExecutionCompletedAsync(JobExecutionCompletion completion, CancellationToken cancellationToken) => Task.CompletedTask;
}
