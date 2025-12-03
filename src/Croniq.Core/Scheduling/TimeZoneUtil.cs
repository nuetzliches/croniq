using System;

namespace Croniq.Core.Scheduling;

internal static class TimeZoneUtil
{
    public static TimeZoneInfo FindTimeZoneById(string timeZoneId)
    {
        return TimeZoneInfo.FindSystemTimeZoneById(timeZoneId);
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
