namespace Croniq.Auth.Core;

/// <summary>Configuration for validating OIDC/JWT bearer tokens.</summary>
public sealed class CroniqOidcOptions
{
    public bool Enabled { get; set; }

    public string Authority { get; set; } = string.Empty;

    public string? MetadataAddress { get; set; }

    public string? Audience { get; set; }

    public bool RequireHttpsMetadata { get; set; } = true;

    public string TenantClaim { get; set; } = "tenant";

    public string[] TenantFallbackClaims { get; set; } = new[] { "tid" };

    public string EnvironmentClaim { get; set; } = "env";

    public string[] EnvironmentFallbackClaims { get; set; } = Array.Empty<string>();

    public string CallerIdClaim { get; set; } = "sub";

    public string[] CallerIdFallbackClaims { get; set; } = new[] { "oid", "preferred_username" };

    public string[] ScopeClaims { get; set; } = new[] { "scope", "scp" };

    public string[] RequiredScopes { get; set; } = Array.Empty<string>();

    public string? DefaultEnvironment { get; set; }

    public bool NormalizeScopesToLowercase { get; set; } = true;

    public int ClockSkewSeconds { get; set; } = 120;

    public TimeSpan MetadataRefreshInterval { get; set; } = TimeSpan.FromHours(6);
}
