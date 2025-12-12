using System;
using System.Collections.Generic;
using Croniq.Core.Scheduling;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Scheduling;

public class SchedulingHelperTests
{
    [Fact]
    public void SortedSetExtensions_respects_wildcards_and_previous_values()
    {
        var set = new SortedSet<int> { 2, 5, 9 };
        set.TailSet(5).Contains(5).ShouldBeTrue();

        set.TryGetMinValueStartingFrom(new DateTimeOffset(2025, 12, 05, 0, 0, 0, TimeSpan.Zero), includePrevious: false, out var min)
            .ShouldBeTrue();
        min.ShouldBe(5);

        var wildcard = new SortedSet<int> { CronExpressionConstants.AllSpec };
        wildcard.TryGetMinValueStartingFrom(new DateTimeOffset(2025, 12, 12, 0, 0, 0, TimeSpan.Zero), includePrevious: false, out var wildcardMin)
            .ShouldBeTrue();
        wildcardMin.ShouldBe(12);

        var withPrevious = new SortedSet<int> { 1, 15, 20 };
        withPrevious.TryGetMinValueStartingFrom(new DateTimeOffset(2025, 12, 12, 0, 0, 0, TimeSpan.Zero), includePrevious: true, out var previousMin)
            .ShouldBeTrue();
        previousMin.ShouldBe(1);

        var empty = new SortedSet<int>();
        empty.TryGetMinValueStartingFrom(DateTimeOffset.UtcNow, includePrevious: false, out _).ShouldBeFalse();
    }

    [Fact]
    public void CronExpressionSummary_formats_values()
    {
        var summary = new CronExpressionSummary(
            new[] { 0 },
            new[] { 30 },
            new[] { 12 },
            new[] { 1, 15 },
            new[] { 1 },
            new[] { 2, 4, 6 },
            lastDayOfWeek: false,
            nearestWeekday: true,
            nthdayOfWeek: 0,
            lastDayOfMonth: false,
            calendarDayOfWeek: false,
            calendarDayOfMonth: false,
            years: new[] { 2025 });

        var text = summary.ToString();
        text.ShouldContain("seconds: 0");
        text.ShouldContain("daysOfMonth: 1,15");
        text.ShouldContain("years: 2025");
    }

    [Fact]
    public void SystemTime_exposes_now_and_utc()
    {
        var now = SystemTime.Now();
        var utc = SystemTime.UtcNow();
        (utc <= DateTimeOffset.UtcNow.AddSeconds(1)).ShouldBeTrue();
        (now <= DateTimeOffset.Now.AddSeconds(1)).ShouldBeTrue();
    }

    [Fact]
    public void ThrowHelper_throws_expected_exceptions()
    {
        Should.Throw<FormatException>(() => Throw.FormatException("bad"));
        Should.Throw<ArgumentException>(() => Throw.ArgumentException("bad", "p"));
        Should.Throw<ArgumentOutOfRangeException>(() => Throw.ArgumentOutOfRangeException<string>("bad", "p"));
        Should.Throw<NotSupportedException>(() => Throw.NotSupportedException("nope"));
    }
}
