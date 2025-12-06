using System.Collections.Generic;

namespace Croniq.Core.Policies;

/// <summary>
/// Container for policy overrides at tenant/namespace/job scope.
/// </summary>
public sealed class PolicyOverrideOptions
{
    public IList<MisfirePolicyOverride> Misfire { get; init; } = new List<MisfirePolicyOverride>();
    public IList<QuotaOverride> Quotas { get; init; } = new List<QuotaOverride>();
    public IList<ExecutionPolicyOverride> Execution { get; init; } = new List<ExecutionPolicyOverride>();
}

public sealed class MisfirePolicyOverride
{
    public string? TenantId { get; init; }
    public string? EnvironmentTag { get; init; }
    public string? NamespaceSegment { get; init; }
    public string? JobName { get; init; }
    public MisfirePolicyOptions Options { get; init; } = new MisfirePolicyOptions();
}

public sealed class QuotaOverride
{
    public string? TenantId { get; init; }
    public string? EnvironmentTag { get; init; }
    public string? NamespaceSegment { get; init; }
    public string? JobName { get; init; }
    public QuotaOptions Options { get; init; } = new QuotaOptions();
}

public sealed class ExecutionPolicyOverride
{
    public string? TenantId { get; init; }
    public string? EnvironmentTag { get; init; }
    public string? NamespaceSegment { get; init; }
    public string? JobName { get; init; }
    public ExecutionPolicyOptions Options { get; init; } = new ExecutionPolicyOptions();
}
