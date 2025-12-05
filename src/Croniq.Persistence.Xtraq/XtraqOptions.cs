namespace Croniq.Persistence.Xtraq;

/// <summary>
/// Connection and schema settings for the Xtraq-backed provider.
/// </summary>
public sealed class XtraqOptions
{
    /// <summary>
    /// Connection string for the CroniqDev (or target) database.
    /// </summary>
    public string ConnectionString { get; set; } = string.Empty;

    /// <summary>
    /// Optional schema name if it differs from defaults.
    /// </summary>
    public string? Schema { get; set; }

    /// <summary>
    /// Actor identifier used for auditing in stored procedures (e.g. "system" or the current principal).
    /// </summary>
    public string Actor { get; set; } = "system";

    /// <summary>
    /// Default lease duration (seconds) when acquiring trigger leases.
    /// </summary>
    public int LeaseDurationSeconds { get; set; } = 60;
}
