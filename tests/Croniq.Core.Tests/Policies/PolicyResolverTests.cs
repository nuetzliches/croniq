using System;
using Croniq.Core.Jobs;
using Croniq.Core.Policies;
using Xunit;

namespace Croniq.Core.Tests.Policies;

public class PolicyResolverTests
{
    [Fact]
    public void Picks_most_specific_misfire_override()
    {
        var resolver = new PolicyResolver(
            Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions { MaxMisfireDelay = TimeSpan.FromMinutes(5), DeadLetterOnMisfire = false }),
            Microsoft.Extensions.Options.Options.Create(new PolicyOverrideOptions
            {
                Misfire =
                {
                    new MisfirePolicyOverride
                    {
                        TenantId = "t1",
                        Options = new MisfirePolicyOptions
                        {
                            MaxMisfireDelay = TimeSpan.FromMinutes(2),
                            DeadLetterOnMisfire = false,
                            RescheduleBackoff = TimeSpan.FromSeconds(10)
                        }
                    },
                    new MisfirePolicyOverride
                    {
                        TenantId = "t1",
                        EnvironmentTag = "dev",
                        NamespaceSegment = "billing",
                        Options = new MisfirePolicyOptions
                        {
                            MaxMisfireDelay = TimeSpan.FromMinutes(1),
                            DeadLetterOnMisfire = true,
                            RescheduleBackoff = TimeSpan.FromSeconds(5)
                        }
                    }
                }
            }));

        var jobKey = new JobKey("t1", "dev", "billing", "invoice");
        var resolved = resolver.ResolveMisfire(jobKey);

        Assert.Equal(TimeSpan.FromMinutes(1), resolved.MaxMisfireDelay);
        Assert.True(resolved.DeadLetterOnMisfire);
        Assert.Equal(TimeSpan.FromSeconds(5), resolved.RescheduleBackoff);
    }

    [Fact]
    public void Applies_most_restrictive_quota_from_overrides()
    {
        var resolver = new PolicyResolver(
            Microsoft.Extensions.Options.Options.Create(new MisfirePolicyOptions()),
            Microsoft.Extensions.Options.Options.Create(new PolicyOverrideOptions
            {
                Quotas =
                {
                    new QuotaOverride
                    {
                        TenantId = "t1",
                        Options = new QuotaOptions { MaxTriggersPerMinute = 80, MaxParallelExecutionsPerJob = 4 }
                    },
                    new QuotaOverride
                    {
                        TenantId = "t1",
                        EnvironmentTag = "dev",
                        NamespaceSegment = "billing",
                        Options = new QuotaOptions { MaxTriggersPerMinute = 50, MaxParallelExecutionsPerJob = 3 }
                    }
                }
            }));

        var jobKey = new JobKey("t1", "dev", "billing", "invoice");
        var resolved = resolver.ResolveQuota(jobKey);

        Assert.Equal(50, resolved.MaxTriggersPerMinute);
        Assert.Equal(3, resolved.MaxParallelExecutionsPerJob);
    }
}
