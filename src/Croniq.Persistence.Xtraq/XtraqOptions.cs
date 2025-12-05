namespace Croniq.Persistence.Xtraq;

/// <summary>Options for configuring the Xtraq persistence provider.</summary>
public sealed class XtraqOptions
{
    /// <summary>Connection string to the Xtraq-backed SQL database.</summary>
    public string? ConnectionString { get; set; }

    /// <summary>Schema name for Croniq objects (default: 'croniq').</summary>
    public string Schema { get; set; } = "croniq";

    /// <summary>Actor used for audit columns and procedure bindings.</summary>
    public string Actor { get; set; } = "croniq-api";

    /// <summary>Lease duration for trigger acquisition (seconds).</summary>
    public int LeaseDurationSeconds { get; set; } = 60;
}
