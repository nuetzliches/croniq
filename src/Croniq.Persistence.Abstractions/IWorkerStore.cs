using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Persists worker host heartbeats and provides availability information.
/// </summary>
public interface IWorkerStore
{
    Task UpsertHeartbeatAsync(WorkerHeartbeat heartbeat, CancellationToken cancellationToken);

    Task<IReadOnlyCollection<WorkerStatus>> ListAsync(WorkerQuery query, CancellationToken cancellationToken);
}
