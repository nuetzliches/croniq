namespace Croniq.Data.SqlServer;

/// <summary>
/// Shared configuration for Croniq's EF Core SQL Server contexts.
/// </summary>
public sealed class SqlServerOptions
{
    /// <summary>Connection string pointing at the Croniq SQL Server database.</summary>
    public string? ConnectionString { get; set; }

    /// <summary>Optional assembly name for EF Core migrations.</summary>
    public string? MigrationsAssembly { get; set; }

    /// <summary>Optional EF Core command timeout in seconds.</summary>
    public int? CommandTimeoutSeconds { get; set; }

    /// <summary>Enable EF Core detailed errors (defaults to false).</summary>
    public bool EnableDetailedErrors { get; set; }

    /// <summary>Enable EF Core sensitive data logging (defaults to false).</summary>
    public bool EnableSensitiveDataLogging { get; set; }

    /// <summary>
    /// Suppresses the EF Core warning "Savepoints are disabled because Multiple Active Result Sets (MARS) is enabled".
    /// This is mainly useful in tests where MARS is enabled but the warning is considered noise.
    /// </summary>
    public bool SuppressMarsSavepointWarning { get; set; }
}
