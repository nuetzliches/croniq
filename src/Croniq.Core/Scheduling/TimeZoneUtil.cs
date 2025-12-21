using System;

namespace Croniq.Core.Scheduling;

internal static class TimeZoneUtil
{
    public static TimeZoneInfo FindTimeZoneById(string timeZoneId)
    {
        return TimeZoneInfo.FindSystemTimeZoneById(timeZoneId);
    }

    public static bool TryFindTimeZoneById(string timeZoneId, out TimeZoneInfo? timeZone)
    {
        timeZone = null;
        if (string.IsNullOrWhiteSpace(timeZoneId))
        {
            return false;
        }

        try
        {
            timeZone = TimeZoneInfo.FindSystemTimeZoneById(timeZoneId);
            return true;
        }
        catch
        {
            return false;
        }
    }

    public static TimeZoneInfo ResolveTimeZone(string? timeZoneId)
    {
        if (string.IsNullOrWhiteSpace(timeZoneId))
        {
            return TimeZoneInfo.Utc;
        }

        return TryFindTimeZoneById(timeZoneId, out var resolved) ? resolved! : TimeZoneInfo.Utc;
    }

    public static DateTimeOffset ConvertTime(DateTimeOffset value, TimeZoneInfo timeZone)
    {
        return TimeZoneInfo.ConvertTime(value, timeZone);
    }

    public static TimeSpan GetUtcOffset(DateTime dateTime, TimeZoneInfo timeZone)
    {
        return timeZone.GetUtcOffset(dateTime);
    }
}
