namespace Croniq.Auth.SqlServer;

public sealed class PasswordAuthOptions
{
    public bool Enabled { get; set; }

    /// <summary>
    /// Optional default tenant reference to use when requests omit tenant information.
    /// In V1 (self-hosted/single-tenant), this should be set to the tenant reference.
    /// </summary>
    public string? DefaultTenant { get; set; }

    public int AccessTokenLifetimeMinutes { get; set; } = 15;

    public int RefreshTokenLifetimeDays { get; set; } = 7;

    public int MaxFailedAccessAttempts { get; set; } = 10;

    public int LockoutMinutes { get; set; } = 15;
}
