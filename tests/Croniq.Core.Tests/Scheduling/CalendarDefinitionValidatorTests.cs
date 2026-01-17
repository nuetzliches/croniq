using System.Collections.Generic;
using Croniq.Core.Scheduling;
using Croniq.Options;
using Croniq.Persistence.Abstractions;
using Shouldly;
using Xunit;

namespace Croniq.Core.Tests.Scheduling;

public sealed class CalendarDefinitionValidatorTests
{
    [Fact]
    public void TryValidate_Fails_for_invalid_time_zone()
    {
        var definition = new CroniqCalendarSeedDefinition
        {
            CalendarId = "cal-1",
            Name = "Calendar",
            TimeZoneId = "Not/A/Zone",
            Mode = CalendarMode.Include
        };

        var ok = CalendarDefinitionValidator.TryValidate(definition, scope: null, out _, out var error);

        ok.ShouldBeFalse();
        error.ShouldBe("TimeZoneId 'Not/A/Zone' is invalid.");
    }

    [Fact]
    public void TryValidate_Fails_for_duplicate_rule_id()
    {
        var rules = new List<CalendarRuleDefinition>
        {
            new(
                "rule-1",
                CalendarRuleType.DailyWindow,
                SortOrder: 0,
                IsEnabled: true,
                DailyWindow: new CalendarDailyWindowRule("09:00", "17:00")),
            new(
                "rule-1",
                CalendarRuleType.WeeklyWindow,
                SortOrder: 1,
                IsEnabled: true,
                WeeklyWindow: new CalendarWeeklyWindowRule(new[] { "Mon" }))
        };

        var definition = new CroniqCalendarSeedDefinition
        {
            CalendarId = "cal-dup",
            Name = "Duplicate Calendar",
            TimeZoneId = "UTC",
            Mode = CalendarMode.Include,
            Rules = rules
        };

        var ok = CalendarDefinitionValidator.TryValidate(definition, scope: null, out _, out var error);

        ok.ShouldBeFalse();
        error.ShouldBe("RuleId 'rule-1' is duplicated.");
    }
}
