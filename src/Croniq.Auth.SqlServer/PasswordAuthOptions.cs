namespace Croniq.Auth.SqlServer;

public sealed class PasswordAuthOptions
{
    public bool Enabled { get; set; }

    public int AccessTokenLifetimeMinutes { get; set; } = 15;

    public int RefreshTokenLifetimeDays { get; set; } = 7;

    public int MaxFailedAccessAttempts { get; set; } = 10;

    public int LockoutMinutes { get; set; } = 15;
}
