using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core;

public sealed class NoOpRunnerStore : IRunnerStore
{
    public Task UpsertHeartbeatAsync(RunnerHeartbeat heartbeat, CancellationToken cancellationToken)
        => Task.CompletedTask;

    public Task<IReadOnlyCollection<RunnerStatus>> ListAsync(RunnerQuery query, CancellationToken cancellationToken)
        => Task.FromResult<IReadOnlyCollection<RunnerStatus>>(System.Array.Empty<RunnerStatus>());

    public Task<RunnerStatus?> TryGetAsync(RunnerLookup lookup, CancellationToken cancellationToken)
        => Task.FromResult<RunnerStatus?>(null);

    public Task<bool> DeleteAsync(RunnerLookup lookup, CancellationToken cancellationToken)
        => Task.FromResult(false);
}
