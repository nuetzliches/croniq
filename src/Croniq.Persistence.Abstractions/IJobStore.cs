using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Minimal trigger store abstraction used by scheduler workers to acquire and release executions.
/// </summary>
public interface IJobStore
{
    Task<IReadOnlyCollection<TriggerLease>> AcquireAsync(TriggerAcquireRequest request, CancellationToken cancellationToken);

    Task<TriggerLease?> TryRenewLeaseAsync(TriggerLeaseRenewRequest request, CancellationToken cancellationToken);

    Task ReleaseAsync(TriggerReleaseRequest request, CancellationToken cancellationToken);

    Task MoveToDeadLetterAsync(DeadLetterRequest request, CancellationToken cancellationToken);
}
