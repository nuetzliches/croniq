using System;

namespace Croniq.Options;

public sealed class WorkerHostOptions
{
    /// <summary>
    /// How many leases to process per batch.
    /// </summary>
    public int BatchSize { get; set; } = 20;

    /// <summary>
    /// Delay when no work was processed.
    /// </summary>
    public TimeSpan IdleDelay { get; set; } = TimeSpan.FromSeconds(2);

    /// <summary>
    /// Delay when work was processed (to throttle tight loops).
    /// </summary>
    public TimeSpan BusyDelay { get; set; } = TimeSpan.FromMilliseconds(250);

    /// <summary>
    /// Delay after an error before retrying.
    /// </summary>
    public TimeSpan ErrorDelay { get; set; } = TimeSpan.FromSeconds(5);
}
