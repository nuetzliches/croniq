using System;

namespace Croniq.Core.Policies;

public sealed class MisfirePolicyOptions
{
    /// <summary>
    /// Maximum allowed delay between scheduled fire time and execution start before a trigger is considered misfired.
    /// Defaults to 5 minutes.
    /// </summary>
    public TimeSpan MaxMisfireDelay { get; set; } = TimeSpan.FromMinutes(5);

    /// <summary>
    /// Whether misfired triggers should be dead-lettered (true) or skipped with next fire scheduled (false).
    /// </summary>
    public bool DeadLetterOnMisfire { get; set; } = true;

    /// <summary>
    /// Optional fixed backoff for rescheduling misfired triggers when DeadLetterOnMisfire is false.
    /// </summary>
    public TimeSpan? RescheduleBackoff { get; set; } = TimeSpan.FromSeconds(30);
}
