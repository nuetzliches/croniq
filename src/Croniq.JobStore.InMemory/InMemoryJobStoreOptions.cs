using System;
using Croniq.Core.Scheduling;

namespace Croniq.JobStore.InMemory;

/// <summary>
/// Configuration for the in-memory job store.
/// </summary>
public sealed class InMemoryJobStoreOptions
{
    internal const int DefaultLeaseDurationSeconds = 60;

    /// <summary>
    /// Lease duration applied when acquiring triggers.
    /// </summary>
    public int LeaseDurationSeconds { get; set; } = DefaultLeaseDurationSeconds;

    /// <summary>
    /// Time provider used to compute initial schedules and dead-letter timestamps (primarily for testing).
    /// </summary>
    public Func<DateTimeOffset>? UtcNowProvider { get; set; } = DefaultUtcNow;

    /// <summary>
    /// Evaluation guard settings for calendar filtering.
    /// </summary>
    public CalendarEvaluationOptions CalendarEvaluation { get; set; } = new();

    internal static DateTimeOffset DefaultUtcNow() => DateTimeOffset.UtcNow;

    internal void Normalize()
    {
        if (LeaseDurationSeconds <= 0)
        {
            LeaseDurationSeconds = DefaultLeaseDurationSeconds;
        }

        UtcNowProvider ??= DefaultUtcNow;
        CalendarEvaluation ??= new CalendarEvaluationOptions();
    }
}
