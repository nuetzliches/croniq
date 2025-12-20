using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Read/replay surface for persisted dead-letter entries.
/// </summary>
public interface IJobDeadLetterStore
{
    Task<IReadOnlyCollection<JobDeadLetterEntry>> ListAsync(PartitionScope scope, CancellationToken cancellationToken);

    Task<JobDeadLetterEntry?> FindAsync(long id, PartitionScope scope, CancellationToken cancellationToken);

    Task ResolveAsync(long id, PartitionScope scope, CancellationToken cancellationToken);
}
