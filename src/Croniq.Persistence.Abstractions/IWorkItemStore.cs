using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

public interface IWorkItemStore
{
    Task UpsertAssignmentAsync(WorkAssignment assignment, CancellationToken cancellationToken);

    Task<bool> TryRenewAsync(WorkLeaseRenewal renewal, CancellationToken cancellationToken);

    Task<bool> TryCompleteAsync(WorkCompletion completion, CancellationToken cancellationToken);
}
