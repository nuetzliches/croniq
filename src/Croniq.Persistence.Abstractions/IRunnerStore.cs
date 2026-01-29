using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Persists runner heartbeats and provides availability information.
/// </summary>
public interface IRunnerStore
{
    Task UpsertHeartbeatAsync(RunnerHeartbeat heartbeat, CancellationToken cancellationToken);

    Task<IReadOnlyCollection<RunnerStatus>> ListAsync(RunnerQuery query, CancellationToken cancellationToken);

    Task<RunnerStatus?> TryGetAsync(RunnerLookup lookup, CancellationToken cancellationToken);

    Task<bool> DeleteAsync(RunnerLookup lookup, CancellationToken cancellationToken);
}
