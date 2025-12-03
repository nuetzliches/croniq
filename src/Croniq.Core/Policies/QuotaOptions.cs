namespace Croniq.Core.Policies;

/// <summary>
/// Quota limits applied per scope. Lower values are more restrictive.
/// </summary>
public sealed class QuotaOptions
{
    /// <summary>
    /// Maximum triggers/requests per minute for a JobKey. Default: 60.
    /// </summary>
    public int MaxTriggersPerMinute { get; set; } = 60;

    /// <summary>
    /// Maximum parallel executions per JobKey. Default: 5.
    /// </summary>
    public int MaxParallelExecutionsPerJob { get; set; } = 5;
}
