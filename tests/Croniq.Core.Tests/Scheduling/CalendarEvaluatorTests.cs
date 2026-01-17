using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Scheduling;

public sealed class CalendarEvaluatorTests
{
    [Fact]
    public void GetNextOccurrence_Includes_candidate_inside_window()
    {
        var scope = new PartitionScope("tenant-a", "dev");
        var trigger = new TriggerDefinition(
            "trigger-1",
            "ops:job",
            "0 0 * * * ?",
            scope,
            TimeZoneId: "UTC");
        var calendar = new CalendarDefinition(
            "cal-1",
            scope.TenantId,
            scope.EnvironmentTag,
            "Work Hours",
            null,
            "UTC",
            CalendarMode.Include,
            new[]
            {
                new CalendarRuleDefinition(
                    "daily",
                    CalendarRuleType.DailyWindow,
                    SortOrder: 0,
                    IsEnabled: true,
                    DailyWindow: new CalendarDailyWindowRule("09:00", "10:00"))
            },
            Enabled: true,
            DateTimeOffset.UtcNow,
            DateTimeOffset.UtcNow);

        var reference = new DateTimeOffset(2026, 1, 1, 8, 0, 0, TimeSpan.Zero);
        var next = CalendarEvaluator.GetNextOccurrence(trigger, reference, calendar, new CalendarEvaluationOptions());

        next.ShouldBe(new DateTimeOffset(2026, 1, 1, 9, 0, 0, TimeSpan.Zero));
    }

    [Fact]
    public void GetNextOccurrence_Returns_null_for_excluded_once()
    {
        var scope = new PartitionScope("tenant-a", "dev");
        var startAt = new DateTimeOffset(2026, 1, 1, 10, 0, 0, TimeSpan.Zero);
        var trigger = new TriggerDefinition(
            "once-trigger",
            "ops:job",
            TriggerSchedule.OnceExpression,
            scope,
            StartAtUtc: startAt,
            TimeZoneId: "UTC");
        var calendar = new CalendarDefinition(
            "cal-exclude",
            scope.TenantId,
            scope.EnvironmentTag,
            "Holiday",
            null,
            "UTC",
            CalendarMode.Exclude,
            new[]
            {
                new CalendarRuleDefinition(
                    "holiday",
                    CalendarRuleType.DateList,
                    SortOrder: 0,
                    IsEnabled: true,
                    DateList: new CalendarDateListRule(new[] { "2026-01-01" }))
            },
            Enabled: true,
            DateTimeOffset.UtcNow,
            DateTimeOffset.UtcNow);

        var next = CalendarEvaluator.GetNextOccurrence(trigger, startAt.AddMinutes(-1), calendar, new CalendarEvaluationOptions());

        next.ShouldBeNull();
    }
}
