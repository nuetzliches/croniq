using System;
using System.Collections.Generic;

namespace Croniq.Persistence.Abstractions;

public enum CalendarMode
{
    Include = 0,
    Exclude = 1
}

public enum CalendarRuleType
{
    DailyWindow = 0,
    WeeklyWindow = 1,
    AnnualDateList = 2,
    DateList = 3,
    CronRule = 4
}

public sealed record CalendarDefinition(
    string CalendarId,
    string TenantId,
    string EnvironmentTag,
    string Name,
    string? Description,
    string TimeZoneId,
    CalendarMode Mode,
    IReadOnlyCollection<CalendarRuleDefinition> Rules,
    bool Enabled,
    DateTimeOffset CreatedAtUtc,
    DateTimeOffset UpdatedAtUtc);

public sealed record CalendarUpsert(
    string CalendarId,
    string TenantId,
    string EnvironmentTag,
    string Name,
    string? Description,
    string TimeZoneId,
    CalendarMode Mode,
    IReadOnlyCollection<CalendarRuleDefinition> Rules,
    bool Enabled);

public sealed record CalendarRuleDefinition(
    string RuleId,
    CalendarRuleType RuleType,
    int SortOrder,
    bool IsEnabled,
    CalendarDailyWindowRule? DailyWindow = null,
    CalendarWeeklyWindowRule? WeeklyWindow = null,
    CalendarAnnualDateListRule? AnnualDateList = null,
    CalendarDateListRule? DateList = null,
    CalendarCronRule? CronRule = null);

public sealed record CalendarDailyWindowRule(
    string StartTime,
    string EndTime,
    IReadOnlyCollection<string>? DaysOfWeek = null);

public sealed record CalendarWeeklyWindowRule(
    IReadOnlyCollection<string> DaysOfWeek);

public sealed record CalendarAnnualDateListRule(
    IReadOnlyCollection<string> MonthDays);

public sealed record CalendarDateListRule(
    IReadOnlyCollection<string> Dates);

public sealed record CalendarCronRule(
    string CronExpression);
