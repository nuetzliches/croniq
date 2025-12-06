namespace Croniq.Api;

public sealed class CroniqPersistenceOptions
{
    public string Mode { get; set; } = "InMemory"; // Xtraq | InMemory

    public XtraqPersistenceOptions Xtraq { get; set; } = new();
}

public sealed class XtraqPersistenceOptions
{
    public string? ConnectionString { get; set; }
}
