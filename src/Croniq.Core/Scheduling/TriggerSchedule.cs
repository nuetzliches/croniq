using System;

namespace Croniq.Core.Scheduling;

public static class TriggerSchedule
{
    public const string OnceExpression = "@once";
    private const string OnceAlias = "once";

    public static bool IsOnceExpression(string? expression)
    {
        if (string.IsNullOrWhiteSpace(expression))
        {
            return false;
        }

        var trimmed = expression.Trim();
        return string.Equals(trimmed, OnceExpression, StringComparison.OrdinalIgnoreCase)
            || string.Equals(trimmed, OnceAlias, StringComparison.OrdinalIgnoreCase);
    }

    public static DateTimeOffset? GetNextOccurrence(
        string expression,
        DateTimeOffset referenceUtc,
        DateTimeOffset? startAtUtc = null,
        DateTimeOffset? endAtUtc = null,
        TimeZoneInfo? timeZone = null)
    {
        if (IsOnceExpression(expression))
        {
            return GetOnceOccurrence(referenceUtc, startAtUtc, endAtUtc);
        }

        var schedule = new CronSchedule(expression, timeZone);
        var cursor = referenceUtc;

        if (startAtUtc.HasValue && startAtUtc.Value > cursor)
        {
            cursor = startAtUtc.Value;
        }

        var next = schedule.GetNextOccurrence(cursor);
        if (next.HasValue && endAtUtc.HasValue && next.Value > endAtUtc.Value)
        {
            return null;
        }

        return next;
    }

    private static DateTimeOffset? GetOnceOccurrence(DateTimeOffset referenceUtc, DateTimeOffset? startAtUtc, DateTimeOffset? endAtUtc)
    {
        var fireAt = startAtUtc ?? referenceUtc;
        if (fireAt < referenceUtc)
        {
            fireAt = referenceUtc;
        }

        if (endAtUtc.HasValue && fireAt > endAtUtc.Value)
        {
            return null;
        }

        return fireAt;
    }
}
