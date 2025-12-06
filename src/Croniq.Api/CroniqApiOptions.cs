namespace Croniq.Api;

/// <summary>
/// API host options.
/// </summary>
public sealed class CroniqApiOptions
{
    /// <summary>
    /// Requests per minute per API key (fixed window).
    /// </summary>
    public int RequestsPerMinute { get; set; } = 60;
}
