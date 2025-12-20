using System;
using Croniq.Core.Scheduling;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Scheduling;

public class TriggerScheduleTests
{
    [Fact]
    public void OnceExpression_UsesStartAtWhenFuture()
    {
        var now = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);
        var startAt = now.AddMinutes(5);

        var next = TriggerSchedule.GetNextOccurrence("@once", now, startAtUtc: startAt);

        next.ShouldBe(startAt);
    }

    [Fact]
    public void OnceExpression_RespectsEndAtBound()
    {
        var now = new DateTimeOffset(2025, 1, 1, 0, 0, 0, TimeSpan.Zero);
        var endAt = now.AddMinutes(-1);

        var next = TriggerSchedule.GetNextOccurrence("@once", now, startAtUtc: null, endAtUtc: endAt);

        next.ShouldBeNull();
    }
}
