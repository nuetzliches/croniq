using System;

namespace Croniq.Persistence.Abstractions;

/// <summary>
/// Options for runner heartbeat tracking.
/// </summary>
public sealed class RunnerStoreOptions
{
    /// <summary>
    /// How long a runner is considered online after its last heartbeat.
    /// </summary>
    public TimeSpan OnlineTtl { get; set; } = TimeSpan.FromSeconds(60);

    /// <summary>
    /// How long to retain runner presence after the last heartbeat.
    /// </summary>
    public TimeSpan OfflineRetentionTtl { get; set; } = TimeSpan.FromMinutes(30);

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

        if (OfflineRetentionTtl <= TimeSpan.Zero)
        {
            OfflineRetentionTtl = TimeSpan.FromMinutes(30);
        }

        if (OfflineRetentionTtl < OnlineTtl)
        {
            OfflineRetentionTtl = OnlineTtl;
        }

        if (OfflineRetentionTtl > TimeSpan.FromDays(7))
        {
            OfflineRetentionTtl = TimeSpan.FromDays(7);
        }
    }
}
