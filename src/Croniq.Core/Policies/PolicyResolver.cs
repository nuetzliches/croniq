using System;
using System.Collections.Generic;
using System.Linq;
using Croniq.Core.Jobs;
using Microsoft.Extensions.Options;

namespace Croniq.Core.Policies;

public sealed class PolicyResolver : IPolicyResolver
{
    private readonly MisfirePolicyOptions _defaultMisfire;
    private readonly IReadOnlyList<MisfirePolicyOverride> _misfireOverrides;
    private readonly QuotaOptions _defaultQuota;
    private readonly IReadOnlyList<QuotaOverride> _quotaOverrides;

    public PolicyResolver(IOptions<MisfirePolicyOptions> defaultMisfire, IOptions<PolicyOverrideOptions> overrides)
    {
        _defaultMisfire = Clone(defaultMisfire?.Value ?? new MisfirePolicyOptions());
        var ov = overrides?.Value ?? new PolicyOverrideOptions();
        _misfireOverrides = ov.Misfire?.ToList() ?? [];
        _defaultQuota = Clone(new QuotaOptions());
        _quotaOverrides = ov.Quotas?.ToList() ?? [];
    }

    public MisfirePolicyOptions ResolveMisfire(JobKey jobKey)
    {
        var result = Clone(_defaultMisfire);
        var match = _misfireOverrides
            .Where(o => Matches(o, jobKey))
            .OrderByDescending(GetSpecificity)
            .FirstOrDefault();

        if (match is not null)
        {
            Apply(result, match.Options);
        }

        return result;
    }

    public QuotaOptions ResolveQuota(JobKey jobKey)
    {
        var result = Clone(_defaultQuota);

        // apply all matching overrides, choosing the most restrictive (min) values
        var matches = _quotaOverrides.Where(o => Matches(o, jobKey));
        foreach (var match in matches)
        {
            result.MaxTriggersPerMinute = Math.Min(result.MaxTriggersPerMinute, match.Options.MaxTriggersPerMinute);
            result.MaxParallelExecutionsPerJob = Math.Min(result.MaxParallelExecutionsPerJob, match.Options.MaxParallelExecutionsPerJob);
        }

        return result;
    }

    private static MisfirePolicyOptions Clone(MisfirePolicyOptions source) =>
        new()
        {
            MaxMisfireDelay = source.MaxMisfireDelay,
            DeadLetterOnMisfire = source.DeadLetterOnMisfire,
            RescheduleBackoff = source.RescheduleBackoff
        };

    private static QuotaOptions Clone(QuotaOptions source) =>
        new()
        {
            MaxTriggersPerMinute = source.MaxTriggersPerMinute,
            MaxParallelExecutionsPerJob = source.MaxParallelExecutionsPerJob
        };

    private static void Apply(MisfirePolicyOptions target, MisfirePolicyOptions overrideOptions)
    {
        target.MaxMisfireDelay = overrideOptions.MaxMisfireDelay;
        target.DeadLetterOnMisfire = overrideOptions.DeadLetterOnMisfire;
        target.RescheduleBackoff = overrideOptions.RescheduleBackoff;
    }

    private static bool Matches(MisfirePolicyOverride o, JobKey jobKey)
    {
        if (o.TenantId is not null && !string.Equals(o.TenantId, jobKey.TenantId, StringComparison.OrdinalIgnoreCase))
            return false;
        if (o.EnvironmentTag is not null && !string.Equals(o.EnvironmentTag, jobKey.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
            return false;
        if (o.NamespaceSegment is not null && !string.Equals(o.NamespaceSegment, jobKey.NamespaceSegment, StringComparison.OrdinalIgnoreCase))
            return false;
        if (o.JobName is not null && !string.Equals(o.JobName, jobKey.JobName, StringComparison.OrdinalIgnoreCase))
            return false;
        return true;
    }

    private static int GetSpecificity(MisfirePolicyOverride o)
    {
        var score = 0;
        if (o.TenantId is not null) score += 1;
        if (o.EnvironmentTag is not null) score += 1;
        if (o.NamespaceSegment is not null) score += 1;
        if (o.JobName is not null) score += 1;
        return score;
    }

    private static bool Matches(QuotaOverride o, JobKey jobKey)
    {
        if (o.TenantId is not null && !string.Equals(o.TenantId, jobKey.TenantId, StringComparison.OrdinalIgnoreCase))
            return false;
        if (o.EnvironmentTag is not null && !string.Equals(o.EnvironmentTag, jobKey.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
            return false;
        if (o.NamespaceSegment is not null && !string.Equals(o.NamespaceSegment, jobKey.NamespaceSegment, StringComparison.OrdinalIgnoreCase))
            return false;
        if (o.JobName is not null && !string.Equals(o.JobName, jobKey.JobName, StringComparison.OrdinalIgnoreCase))
            return false;
        return true;
    }
}
