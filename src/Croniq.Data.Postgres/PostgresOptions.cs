namespace Croniq.Data.Postgres;

/// <summary>
/// Shared configuration for Croniq's EF Core Postgres contexts.
/// </summary>
public sealed class PostgresOptions
{
    /// <summary>Connection string pointing at the Croniq Postgres database.</summary>
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
    /// Optional search path applied to the connection (for example, "croniq,auth,public").
    /// </summary>
    public string? SearchPath { get; set; }
}
