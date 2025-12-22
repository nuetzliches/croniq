namespace Croniq.Auth.Core;

/// <summary>Configuration for Croniq-issued bearer tokens.</summary>
public sealed class CroniqTokenOptions
{
    /// <summary>Enables the built-in token issuer. Disabled hosts will reject issuance requests.</summary>
    public bool Enabled { get; set; } = true;

    /// <summary>JWT issuer value embedded in <c>iss</c>.</summary>
    public string Issuer { get; set; } = "https://cronqi.local";

    /// <summary>Default audience when callers do not provide one.</summary>
    public string? DefaultAudience { get; set; } = "cronqi-api";

    /// <summary>Base64 encoded symmetric signing key (HMAC-SHA256).</summary>
    public string SigningKey { get; set; } = string.Empty;

    /// <summary>Default token lifetime in minutes when callers omit <c>ttlMinutes</c>.</summary>
    public int DefaultLifetimeMinutes { get; set; } = 15;

    /// <summary>Claim name for the tenant identifier.</summary>
    public string TenantClaim { get; set; } = "tenant";

    /// <summary>Claim name for the environment tag.</summary>
    public string EnvironmentClaim { get; set; } = "env";

    /// <summary>
    /// Optional default environment tag used when tokens omit the environment claim.
    /// This enables UI flows where the client does not select an environment and the server applies a default.
    /// </summary>
    public string? DefaultEnvironment { get; set; }

    /// <summary>Claim name for the client identifier.</summary>
    public string ClientClaim { get; set; } = "cid";

    /// <summary>Claim name for the Croniq scope list.</summary>
    public string ScopeClaim { get; set; } = "scope";
}
