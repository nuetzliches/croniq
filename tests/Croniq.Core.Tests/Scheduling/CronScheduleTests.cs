using System;
using Croniq.Core.Scheduling;
using FluentAssertions;
using Xunit;

namespace Croniq.Core.Tests.Scheduling;

public class CronScheduleTests
{
    [Fact]
    public void NextOccurrence_Utc_EveryMinute()
    {
        var schedule = new CronSchedule("0 * * * * ?");
        var start = new DateTimeOffset(2025, 1, 1, 12, 0, 30, TimeSpan.Zero);

        var next = schedule.GetNextOccurrence(start);

        next.Should().Be(new DateTimeOffset(2025, 1, 1, 12, 1, 0, TimeSpan.Zero));
    }

    [Fact]
    public void NextOccurrences_WithTimeZone_DailyAtNineBerlin()
    {
        var tz = TimeZoneInfo.FindSystemTimeZoneById("W. Europe Standard Time");
        var schedule = new CronSchedule("0 0 9 * * ?", tz);
        var start = new DateTimeOffset(2025, 6, 1, 7, 0, 0, TimeSpan.Zero); // 09:00 local is 07:00 UTC

        var nextTwo = schedule.GetNextOccurrences(start, 2);

        nextTwo.Should().HaveCount(2);
        nextTwo[0].Should().Be(new DateTimeOffset(2025, 6, 2, 7, 0, 0, TimeSpan.Zero));
        nextTwo[1].Should().Be(new DateTimeOffset(2025, 6, 3, 7, 0, 0, TimeSpan.Zero));
    }
}
