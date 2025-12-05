namespace Croniq.Api;

/// <summary>
/// API host options.
/// </summary>
public sealed class CroniqApiOptions
{
    /// <summary>
    /// Shared API key required via 'X-Croniq-Key' header. Leave empty to disable.
    /// </summary>
    public string ApiKey { get; set; } = "dev-key";

    /// <summary>
    /// Requests per minute per API key (fixed window).
    /// </summary>
    public int RequestsPerMinute { get; set; } = 60;
}
