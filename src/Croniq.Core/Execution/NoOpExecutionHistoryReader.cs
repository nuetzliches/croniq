using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core.Execution;

/// <summary>
/// Fallback implementation used when execution persistence is disabled.
/// </summary>
public sealed class NoOpExecutionHistoryReader : IExecutionHistoryReader
{
    private static readonly IReadOnlyList<ExecutionSummary> Empty = Array.Empty<ExecutionSummary>();

    public Task<IReadOnlyList<ExecutionSummary>> ListExecutionsAsync(PartitionScope scope, ExecutionHistoryQuery? query, CancellationToken cancellationToken)
        => Task.FromResult(Empty);

    public Task<ExecutionSummary?> GetExecutionAsync(string executionId, CancellationToken cancellationToken)
        => Task.FromResult<ExecutionSummary?>(null);
}
