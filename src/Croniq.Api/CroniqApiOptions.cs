using System;
using System.Collections.Generic;

namespace Croniq.Api;

/// <summary>
/// API host options.
/// </summary>
public sealed class CroniqApiOptions
{
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
    /// How long to keep rate limiter entries without activity. Default: 10 minutes.
    /// </summary>
    public TimeSpan RateLimiterCacheRetention { get; set; } = TimeSpan.FromMinutes(10);

    /// <summary>
    /// How often to sweep stale rate limiter entries. Default: 2 minutes.
    /// </summary>
    public TimeSpan RateLimiterCacheCleanupInterval { get; set; } = TimeSpan.FromMinutes(2);

    // Intentionally no "known environments" list: environment selection is token-bound and not discoverable via API.
}

public sealed class TenantRateLimitOptions
{
    /// <summary>
    /// Requests per minute for the tenant (fixed window). Values &lt;=0 fall back to the global limit.
    /// </summary>
    public int RequestsPerMinute { get; set; } = 60;
}
