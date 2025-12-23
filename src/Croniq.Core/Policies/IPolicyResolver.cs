using Croniq.Core.Jobs;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core.Policies;

public interface IPolicyResolver
{
    MisfirePolicyOptions ResolveMisfire(JobKey jobKey, PartitionScope? scope = null);

    QuotaOptions ResolveQuota(JobKey jobKey, PartitionScope? scope = null);

    ExecutionPolicyOptions ResolveExecution(JobKey jobKey, PartitionScope? scope = null);
}
