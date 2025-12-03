using System;

namespace Croniq.Core.Scheduling;

internal static class TriggerConstants
{
    public const int DefaultPriority = 5;
    internal static readonly int YearToGiveUpSchedulingAt = DateTime.UtcNow.Year + 100;
    internal const int EarliestYear = 1970;
}
