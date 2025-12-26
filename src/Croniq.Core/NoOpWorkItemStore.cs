using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core;

public sealed class NoOpWorkItemStore : IWorkItemStore
{
    public Task UpsertAssignmentAsync(WorkAssignment assignment, CancellationToken cancellationToken)
        => Task.CompletedTask;

    public Task<bool> TryRenewAsync(WorkLeaseRenewal renewal, CancellationToken cancellationToken)
        => Task.FromResult(true);

    public Task<bool> TryCompleteAsync(WorkCompletion completion, CancellationToken cancellationToken)
        => Task.FromResult(true);
}
