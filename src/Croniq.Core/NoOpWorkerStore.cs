using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core;

public sealed class NoOpWorkerStore : IWorkerStore
{
    public Task UpsertHeartbeatAsync(WorkerHeartbeat heartbeat, CancellationToken cancellationToken)
        => Task.CompletedTask;

    public Task<IReadOnlyCollection<WorkerStatus>> ListAsync(WorkerQuery query, CancellationToken cancellationToken)
        => Task.FromResult<IReadOnlyCollection<WorkerStatus>>(System.Array.Empty<WorkerStatus>());
}
