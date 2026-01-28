using System;
using System.Collections.Generic;
using System.Linq;
using Croniq.Persistence.Abstractions;

namespace Croniq.Api;

/// <summary>
/// API host options.
/// </summary>
public sealed class CroniqApiOptions
{
    /// <summary>
    /// Limits which API surfaces are exposed by the host.
    /// </summary>
    public CroniqApiSurface Surface { get; set; } = CroniqApiSurface.Full;

    /// <summary>
    /// Requests per minute per API key (fixed window).
    /// </summary>
    public int RequestsPerMinute { get; set; } = 60;

    /// <summary>
    /// Optional path prefixes that skip auth middleware. Use only for development routes (e.g., Swagger) and keep empty in production.
    /// </summary>
    public List<string> AnonymousPathPrefixes { get; set; } = new();

    /// <summary>
    /// Optional per-tenant overrides keyed by TenantId.
    /// </summary>
    public Dictionary<string, TenantRateLimitOptions> TenantRateLimits { get; set; } = new(StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// Optional CIDR allowlist for inbound requests.
    /// </summary>
    public List<string> AllowedIpCidrs { get; set; } = new();

    /// <summary>
    /// How long to keep rate limiter entries without activity. Default: 10 minutes.
    /// </summary>
    public TimeSpan RateLimiterCacheRetention { get; set; } = TimeSpan.FromMinutes(10);

    /// <summary>
    /// How often to sweep stale rate limiter entries. Default: 2 minutes.
    /// </summary>
    public TimeSpan RateLimiterCacheCleanupInterval { get; set; } = TimeSpan.FromMinutes(2);

    /// <summary>
    /// Runner self-registration policy for jobs (pending approval by default).
    /// </summary>
    public RunnerJobRegistrationOptions RunnerJobRegistration { get; set; } = new();

    // Intentionally no "known environments" list: environment selection is token-bound and not discoverable via API.
}

public enum CroniqApiSurface
{
    Full,
    WebhookAdminOnly
}

public sealed class TenantRateLimitOptions
{
    /// <summary>
    /// Requests per minute for the tenant (fixed window). Values &lt;=0 fall back to the global limit.
    /// </summary>
    public int RequestsPerMinute { get; set; } = 60;
}

public enum RunnerJobRegistrationPolicy
{
    RequireApproval,
    AutoActivate,
    Deny
}

public sealed class RunnerJobRegistrationOptions
{
    /// <summary>
    /// Default policy when no override matches. Default: RequireApproval.
    /// </summary>
    public RunnerJobRegistrationPolicy DefaultPolicy { get; set; } = RunnerJobRegistrationPolicy.RequireApproval;

    /// <summary>
    /// Optional policy overrides (tenant-only or tenant+environment).
    /// </summary>
    public List<RunnerJobRegistrationOverride> Overrides { get; set; } = new();

    public RunnerJobRegistrationPolicy Resolve(PartitionScope scope)
    {
        if (scope.TenantId is null || scope.EnvironmentTag is null)
        {
            return DefaultPolicy;
        }

        var tenantId = scope.TenantId.Trim();
        var environment = scope.EnvironmentTag.Trim();

        var environmentMatch = Overrides
            .FirstOrDefault(o => o.Matches(tenantId, environment, requireEnvironment: true));
        if (environmentMatch is not null)
        {
            return environmentMatch.Policy;
        }

        var tenantMatch = Overrides
            .FirstOrDefault(o => o.Matches(tenantId, environment, requireEnvironment: false));
        return tenantMatch?.Policy ?? DefaultPolicy;
    }
}

public sealed class RunnerJobRegistrationOverride
{
    public string TenantId { get; set; } = string.Empty;
    public string? EnvironmentTag { get; set; }
    public RunnerJobRegistrationPolicy Policy { get; set; } = RunnerJobRegistrationPolicy.RequireApproval;

    internal bool Matches(string tenantId, string environmentTag, bool requireEnvironment)
    {
        if (string.IsNullOrWhiteSpace(TenantId))
        {
            return false;
        }

        if (!string.Equals(tenantId, TenantId.Trim(), StringComparison.OrdinalIgnoreCase))
        {
            return false;
        }

        if (requireEnvironment && string.IsNullOrWhiteSpace(EnvironmentTag))
        {
            return false;
        }

        if (string.IsNullOrWhiteSpace(EnvironmentTag))
        {
            return true;
        }

        return string.Equals(environmentTag, EnvironmentTag.Trim(), StringComparison.OrdinalIgnoreCase);
    }
}
