using System;
using System.Collections.Generic;

namespace Croniq.Core.Scheduling;

/// <summary>
/// Thin wrapper around CronExpression for Quartz-compatible cron schedules.
/// </summary>
public sealed class CronSchedule
{
    private readonly CronExpression _expression;

    public CronSchedule(string expression, TimeZoneInfo? timeZone = null)
    {
        if (string.IsNullOrWhiteSpace(expression))
        {
            throw new ArgumentException("Cron expression cannot be null or empty.", nameof(expression));
        }

        _expression = new CronExpression(expression.Trim())
        {
            TimeZone = timeZone ?? TimeZoneInfo.Utc
        };
    }

    public string Expression => _expression.CronExpressionString;

    public TimeZoneInfo TimeZone => _expression.TimeZone;

    public DateTimeOffset? GetNextOccurrence(DateTimeOffset fromUtc)
    {
        return _expression.GetNextValidTimeAfter(fromUtc);
    }

    public IReadOnlyList<DateTimeOffset> GetNextOccurrences(DateTimeOffset fromUtc, int count)
    {
        if (count <= 0) throw new ArgumentOutOfRangeException(nameof(count), "Count must be greater than zero.");

        var result = new List<DateTimeOffset>(count);
        var cursor = fromUtc;

        for (var i = 0; i < count; i++)
        {
            var next = _expression.GetNextValidTimeAfter(cursor);
            if (next is null)
            {
                break;
            }

            result.Add(next.Value);
            cursor = next.Value;
        }

        return result;
    }
}
