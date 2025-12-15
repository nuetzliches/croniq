using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Persisted store abstraction responsible for definitions and trigger lifecycle.
/// </summary>
public interface IJobPersistenceProvider : IJobStore
{
    Task UpsertJobAsync(JobDefinition job, CancellationToken cancellationToken);

    Task<IReadOnlyCollection<JobDefinition>> ListJobsAsync(PartitionScope scope, CancellationToken cancellationToken);

    Task<JobDefinition?> GetJobAsync(string jobKey, CancellationToken cancellationToken);

    Task DeleteJobAsync(string jobKey, PartitionScope scope, CancellationToken cancellationToken);

    Task UpsertTriggerAsync(TriggerDefinition trigger, CancellationToken cancellationToken);

    Task<IReadOnlyCollection<TriggerDefinition>> ListTriggersAsync(PartitionScope scope, CancellationToken cancellationToken);

    Task DeleteTriggerAsync(string triggerId, PartitionScope scope, CancellationToken cancellationToken);
}
