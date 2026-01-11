namespace Croniq.Api;

public sealed class CroniqPersistenceOptions
{
    public string Mode { get; set; } = "InMemory"; // SqlServer | InMemory

    public SqlServerPersistenceNode SqlServer { get; set; } = new();
}

public sealed class SqlServerPersistenceNode
{
    public string? ConnectionString { get; set; }

    public string? MigrationsAssembly { get; set; }

    public bool? EnableDetailedErrors { get; set; }

    public bool? EnableSensitiveDataLogging { get; set; }

    public int? CommandTimeoutSeconds { get; set; }

    public int? LeaseDurationSeconds { get; set; }

    public int? DeadLetterRetentionDays { get; set; }

    public int? DeadLetterReasonMaxLength { get; set; }
}
