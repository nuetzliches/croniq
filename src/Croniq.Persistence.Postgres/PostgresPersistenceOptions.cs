using System;

namespace Croniq.Persistence.Postgres;

/// <summary>
/// Provider-specific knobs for EF Core based persistence.
/// </summary>
public sealed class PostgresPersistenceOptions
{
    private const int DefaultLeaseDurationSeconds = 60;
    private const int MaxReasonLength = 256;
    private const int MaxRetentionDays = 3650;

    public int LeaseDurationSeconds { get; set; } = DefaultLeaseDurationSeconds;

    public int DeadLetterReasonMaxLength { get; set; } = MaxReasonLength;

    public int DeadLetterRetentionDays { get; set; } = 30;

    public void Normalize()
    {
        if (LeaseDurationSeconds <= 0)
        {
            LeaseDurationSeconds = DefaultLeaseDurationSeconds;
        }

        DeadLetterReasonMaxLength = Math.Clamp(DeadLetterReasonMaxLength, 32, MaxReasonLength);
        DeadLetterRetentionDays = Math.Clamp(DeadLetterRetentionDays, 1, MaxRetentionDays);
    }
}
