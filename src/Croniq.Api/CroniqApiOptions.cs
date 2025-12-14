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
}

public sealed class TenantRateLimitOptions
{
    /// <summary>
    /// Requests per minute for the tenant (fixed window). Values &lt;=0 fall back to the global limit.
    /// </summary>
    public int RequestsPerMinute { get; set; } = 60;
}
