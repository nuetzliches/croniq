using System;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Options for worker host heartbeat tracking.
/// </summary>
public sealed class WorkerStoreOptions
{
    /// <summary>
    /// How long a worker instance is considered online after its last heartbeat.
    /// </summary>
    public TimeSpan OnlineTtl { get; set; } = TimeSpan.FromSeconds(60);

    public void Normalize()
    {
        if (OnlineTtl <= TimeSpan.Zero)
        {
            OnlineTtl = TimeSpan.FromSeconds(60);
        }

        if (OnlineTtl > TimeSpan.FromDays(1))
        {
            OnlineTtl = TimeSpan.FromDays(1);
        }
    }
}
