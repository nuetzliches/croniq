using System;
using System.Collections.Generic;

namespace Croniq.Core.Scheduling;

internal static class SortedSetExtensions
{
    public static SortedSet<int> TailSet(this SortedSet<int> set, int value)
    {
        return set.GetViewBetween(value, int.MaxValue);
    }

    public static bool TryGetMinValueStartingFrom(this SortedSet<int> set, DateTimeOffset reference, bool includePrevious, out int min)
    {
        min = 0;
        if (set is null || set.Count == 0)
        {
            return false;
        }

        // Wildcard support: AllSpec/NoSpec behave like "any", so return current day.
        if (set.Contains(CronExpressionConstants.AllSpec) || set.Contains(CronExpressionConstants.NoSpec))
        {
            min = reference.Day;
            return true;
        }

        var start = reference.Day;
        if (includePrevious && start > 1 && set.Min < start)
        {
            // when set already includes a value before start and the caller allows it, return that min
            min = set.Min;
            return true;
        }

        foreach (var value in set)
        {
            if (value >= start)
            {
                min = value;
                return true;
            }
        }

        return false;
    }
}
