using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core.Execution;

/// <summary>
/// Provides summarized execution history for tenants.
/// </summary>
public interface IExecutionHistoryReader
{
    Task<IReadOnlyList<ExecutionSummary>> ListExecutionsAsync(PartitionScope scope, ExecutionHistoryQuery? query, CancellationToken cancellationToken);

    Task<ExecutionSummary?> GetExecutionAsync(string executionId, CancellationToken cancellationToken);
}
