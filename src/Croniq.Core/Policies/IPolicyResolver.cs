using Croniq.Core.Jobs;

namespace Croniq.Core.Policies;

public interface IPolicyResolver
{
    MisfirePolicyOptions ResolveMisfire(JobKey jobKey);

    QuotaOptions ResolveQuota(JobKey jobKey);
}
