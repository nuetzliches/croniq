using Croniq.Auth.Core;

namespace Croniq.Hosting;

public sealed class CroniqAuthOptions
{
    public string Mode { get; set; } = "SqlServer"; // SqlServer | InMemory

    public SqlServerAuthOptions SqlServer { get; set; } = new();

    public InMemoryAuthOptions InMemory { get; set; } = new();

    public CroniqOidcOptions Oidc { get; set; } = new();
}

public sealed class SqlServerAuthOptions
{
    public string? ConnectionString { get; set; }
    public string? MigrationsAssembly { get; set; }
    public bool? EnableDetailedErrors { get; set; }
    public bool? EnableSensitiveDataLogging { get; set; }
}

public sealed class InMemoryAuthOptions
{
    public string ApiKey { get; set; } = "dev-key";
    public string TenantId { get; set; } = "default";
    public string EnvironmentTag { get; set; } = "dev";
}
