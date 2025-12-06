namespace Croniq.Api;

public sealed class CroniqAuthOptions
{
    public string Mode { get; set; } = "Xtraq"; // Xtraq | InMemory

    public XtraqAuthOptions Xtraq { get; set; } = new();

    public InMemoryAuthOptions InMemory { get; set; } = new();
}

public sealed class XtraqAuthOptions
{
    public string? ConnectionString { get; set; }
}

public sealed class InMemoryAuthOptions
{
    public string ApiKey { get; set; } = "dev-key";
    public string TenantId { get; set; } = "dev";
    public string EnvironmentTag { get; set; } = "dev";
}
