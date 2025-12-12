using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Core.Execution;

/// <summary>
/// Abstraction for persisting job execution metadata and log entries.
/// </summary>
public interface IJobLogStore
{
    Task OnExecutionStartedAsync(JobExecutionRecord record, CancellationToken cancellationToken);

    Task AppendAsync(string executionId, IReadOnlyCollection<JobLogEntry> entries, CancellationToken cancellationToken);

    Task OnExecutionCompletedAsync(JobExecutionCompletion completion, CancellationToken cancellationToken);
}
