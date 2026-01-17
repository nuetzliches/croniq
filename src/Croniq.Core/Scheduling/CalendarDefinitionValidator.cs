using System;
using System.Collections.Generic;
using System.Globalization;
using Croniq.Options;
using Croniq.Persistence.Abstractions;

namespace Croniq.Core.Scheduling;

public static class CalendarDefinitionValidator
{
    private const int MaxCalendarIdLength = 128;
    private const int MaxNameLength = 256;
    private const int MaxDescriptionLength = 1024;
    private static readonly string[] TimeFormats = { "hh\\:mm", "h\\:mm", "hh\\:mm\\:ss", "h\\:mm\\:ss" };

    public static bool TryValidate(
        CroniqCalendarSeedDefinition definition,
        PartitionScope? scope,
        out CalendarDefinitionValidationResult result,
        out string? error)
    {
        result = default!;
        error = null;

        var calendarId = definition.CalendarId?.Trim();
        if (string.IsNullOrWhiteSpace(calendarId))
        {
            error = "CalendarId is required.";
            return false;
        }

        if (calendarId.Length > MaxCalendarIdLength)
        {
            error = $"CalendarId must be {MaxCalendarIdLength} characters or fewer.";
            return false;
        }

        var name = definition.Name?.Trim();
        if (string.IsNullOrWhiteSpace(name))
        {
            error = "Name is required.";
            return false;
        }

        if (name.Length > MaxNameLength)
        {
            error = $"Name must be {MaxNameLength} characters or fewer.";
            return false;
        }

        var description = string.IsNullOrWhiteSpace(definition.Description)
            ? null
            : definition.Description.Trim();

        if (description is not null && description.Length > MaxDescriptionLength)
        {
            error = $"Description must be {MaxDescriptionLength} characters or fewer.";
            return false;
        }

        var timeZoneId = definition.TimeZoneId?.Trim();
        if (string.IsNullOrWhiteSpace(timeZoneId))
        {
            error = "TimeZoneId is required.";
            return false;
        }

        if (!TimeZoneUtil.TryFindTimeZoneById(timeZoneId, out var timeZone))
        {
            error = $"TimeZoneId '{timeZoneId}' is invalid.";
            return false;
        }

        _ = scope;

        if (!Enum.IsDefined(typeof(CalendarMode), definition.Mode))
        {
            error = "Mode must be Include or Exclude.";
            return false;
        }

        var normalizedRules = new List<CalendarRuleDefinition>();
        var ruleIds = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        var rules = definition.Rules ?? new List<CalendarRuleDefinition>();

        foreach (var rule in rules)
        {
            if (!TryValidateRule(rule, ruleIds, out var normalized, out error))
            {
                return false;
            }

            normalizedRules.Add(normalized);
        }

        result = new CalendarDefinitionValidationResult(
            calendarId,
            name,
            description,
            timeZone!.Id,
            definition.Mode,
            normalizedRules);

        return true;
    }

    private static bool TryValidateRule(
        CalendarRuleDefinition rule,
        HashSet<string> ruleIds,
        out CalendarRuleDefinition normalized,
        out string? error)
    {
        normalized = rule;
        error = null;

        var ruleId = rule.RuleId?.Trim();
        if (string.IsNullOrWhiteSpace(ruleId))
        {
            error = "RuleId is required for every calendar rule.";
            return false;
        }

        if (!ruleIds.Add(ruleId))
        {
            error = $"RuleId '{ruleId}' is duplicated.";
            return false;
        }

        if (rule.SortOrder < 0)
        {
            error = $"SortOrder must be zero or greater for rule '{ruleId}'.";
            return false;
        }

        if (!Enum.IsDefined(typeof(CalendarRuleType), rule.RuleType))
        {
            error = $"RuleType is invalid for rule '{ruleId}'.";
            return false;
        }

        switch (rule.RuleType)
        {
            case CalendarRuleType.DailyWindow:
                if (rule.DailyWindow is null)
                {
                    error = $"DailyWindow payload is required for rule '{ruleId}'.";
                    return false;
                }

                if (!TryValidateDailyWindow(rule.DailyWindow, out var dailyWindow, out error))
                {
                    error = $"DailyWindow for rule '{ruleId}' is invalid: {error}";
                    return false;
                }

                normalized = rule with { RuleId = ruleId, DailyWindow = dailyWindow, WeeklyWindow = null, AnnualDateList = null, DateList = null, CronRule = null };
                return true;
            case CalendarRuleType.WeeklyWindow:
                if (rule.WeeklyWindow is null)
                {
                    error = $"WeeklyWindow payload is required for rule '{ruleId}'.";
                    return false;
                }

                if (!TryValidateWeeklyWindow(rule.WeeklyWindow, out var weeklyWindow, out error))
                {
                    error = $"WeeklyWindow for rule '{ruleId}' is invalid: {error}";
                    return false;
                }

                normalized = rule with { RuleId = ruleId, WeeklyWindow = weeklyWindow, DailyWindow = null, AnnualDateList = null, DateList = null, CronRule = null };
                return true;
            case CalendarRuleType.AnnualDateList:
                if (rule.AnnualDateList is null)
                {
                    error = $"AnnualDateList payload is required for rule '{ruleId}'.";
                    return false;
                }

                if (!TryValidateAnnualDateList(rule.AnnualDateList, out var annualDates, out error))
                {
                    error = $"AnnualDateList for rule '{ruleId}' is invalid: {error}";
                    return false;
                }

                normalized = rule with { RuleId = ruleId, AnnualDateList = annualDates, DailyWindow = null, WeeklyWindow = null, DateList = null, CronRule = null };
                return true;
            case CalendarRuleType.DateList:
                if (rule.DateList is null)
                {
                    error = $"DateList payload is required for rule '{ruleId}'.";
                    return false;
                }

                if (!TryValidateDateList(rule.DateList, out var dateList, out error))
                {
                    error = $"DateList for rule '{ruleId}' is invalid: {error}";
                    return false;
                }

                normalized = rule with { RuleId = ruleId, DateList = dateList, DailyWindow = null, WeeklyWindow = null, AnnualDateList = null, CronRule = null };
                return true;
            case CalendarRuleType.CronRule:
                if (rule.CronRule is null)
                {
                    error = $"CronRule payload is required for rule '{ruleId}'.";
                    return false;
                }

                if (!TryValidateCronRule(rule.CronRule, out var cronRule, out error))
                {
                    error = $"CronRule for rule '{ruleId}' is invalid: {error}";
                    return false;
                }

                normalized = rule with { RuleId = ruleId, CronRule = cronRule, DailyWindow = null, WeeklyWindow = null, AnnualDateList = null, DateList = null };
                return true;
            default:
                error = $"Unsupported RuleType '{rule.RuleType}' for rule '{ruleId}'.";
                return false;
        }
    }

    private static bool TryValidateDailyWindow(
        CalendarDailyWindowRule rule,
        out CalendarDailyWindowRule normalized,
        out string error)
    {
        error = string.Empty;
        normalized = rule;

        var startTime = rule.StartTime?.Trim();
        var endTime = rule.EndTime?.Trim();
        if (string.IsNullOrWhiteSpace(startTime) || string.IsNullOrWhiteSpace(endTime))
        {
            error = "StartTime and EndTime are required.";
            return false;
        }

        if (!TryParseTimeOfDay(startTime, out _))
        {
            error = $"StartTime '{startTime}' must use HH:mm or HH:mm:ss format.";
            return false;
        }

        if (!TryParseTimeOfDay(endTime, out _))
        {
            error = $"EndTime '{endTime}' must use HH:mm or HH:mm:ss format.";
            return false;
        }

        var days = NormalizeStringList(rule.DaysOfWeek);
        if (!TryValidateDaysOfWeek(days, out var dayError))
        {
            error = dayError;
            return false;
        }

        normalized = rule with { StartTime = startTime, EndTime = endTime, DaysOfWeek = days.Count == 0 ? null : days };
        return true;
    }

    private static bool TryValidateWeeklyWindow(
        CalendarWeeklyWindowRule rule,
        out CalendarWeeklyWindowRule normalized,
        out string error)
    {
        error = string.Empty;
        normalized = rule;

        var days = NormalizeStringList(rule.DaysOfWeek);
        if (days.Count == 0)
        {
            error = "DaysOfWeek must include at least one entry.";
            return false;
        }

        if (!TryValidateDaysOfWeek(days, out error))
        {
            return false;
        }

        normalized = rule with { DaysOfWeek = days };
        return true;
    }

    private static bool TryValidateAnnualDateList(
        CalendarAnnualDateListRule rule,
        out CalendarAnnualDateListRule normalized,
        out string error)
    {
        error = string.Empty;
        normalized = rule;

        var dates = NormalizeStringList(rule.MonthDays);
        if (dates.Count == 0)
        {
            error = "MonthDays must include at least one entry.";
            return false;
        }

        foreach (var value in dates)
        {
            if (!IsValidMonthDay(value))
            {
                error = $"MonthDay '{value}' must use MM-dd format.";
                return false;
            }
        }

        normalized = rule with { MonthDays = dates };
        return true;
    }

    private static bool TryValidateDateList(
        CalendarDateListRule rule,
        out CalendarDateListRule normalized,
        out string error)
    {
        error = string.Empty;
        normalized = rule;

        var dates = NormalizeStringList(rule.Dates);
        if (dates.Count == 0)
        {
            error = "Dates must include at least one entry.";
            return false;
        }

        foreach (var value in dates)
        {
            if (!DateOnly.TryParseExact(value, "yyyy-MM-dd", CultureInfo.InvariantCulture, DateTimeStyles.None, out _))
            {
                error = $"Date '{value}' must use yyyy-MM-dd format.";
                return false;
            }
        }

        normalized = rule with { Dates = dates };
        return true;
    }

    private static bool TryValidateCronRule(
        CalendarCronRule rule,
        out CalendarCronRule normalized,
        out string error)
    {
        error = string.Empty;
        normalized = rule;

        var expression = rule.CronExpression?.Trim();
        if (string.IsNullOrWhiteSpace(expression))
        {
            error = "CronExpression is required.";
            return false;
        }

        if (TriggerSchedule.IsOnceExpression(expression))
        {
            error = "CronExpression cannot use @once.";
            return false;
        }

        try
        {
            _ = new CronExpression(expression);
        }
        catch (Exception ex)
        {
            error = $"CronExpression '{expression}' is invalid ({ex.Message}).";
            return false;
        }

        normalized = rule with { CronExpression = expression };
        return true;
    }

    private static bool TryParseTimeOfDay(string value, out TimeSpan timeOfDay)
    {
        return TimeSpan.TryParseExact(value, TimeFormats, CultureInfo.InvariantCulture, out timeOfDay);
    }

    private static IReadOnlyCollection<string> NormalizeStringList(IReadOnlyCollection<string>? values)
    {
        if (values is null || values.Count == 0)
        {
            return Array.Empty<string>();
        }

        var list = new List<string>(values.Count);
        foreach (var value in values)
        {
            if (string.IsNullOrWhiteSpace(value))
            {
                continue;
            }

            list.Add(value.Trim());
        }

        return list;
    }

    private static bool TryValidateDaysOfWeek(IReadOnlyCollection<string> values, out string error)
    {
        error = string.Empty;
        if (values.Count == 0)
        {
            return true;
        }

        foreach (var value in values)
        {
            if (!TryParseDayOfWeek(value, out _))
            {
                error = $"DaysOfWeek entry '{value}' is invalid.";
                return false;
            }
        }

        return true;
    }

    private static bool TryParseDayOfWeek(string value, out DayOfWeek dayOfWeek)
    {
        if (Enum.TryParse(value, ignoreCase: true, out dayOfWeek))
        {
            return true;
        }

        if (int.TryParse(value, out var numeric) && numeric is >= 0 and <= 6)
        {
            dayOfWeek = (DayOfWeek)numeric;
            return true;
        }

        var trimmed = value.Trim();
        if (trimmed.Length >= 3)
        {
            var prefix = trimmed[..3].ToLowerInvariant();
            switch (prefix)
            {
                case "sun":
                    dayOfWeek = DayOfWeek.Sunday;
                    return true;
                case "mon":
                    dayOfWeek = DayOfWeek.Monday;
                    return true;
                case "tue":
                    dayOfWeek = DayOfWeek.Tuesday;
                    return true;
                case "wed":
                    dayOfWeek = DayOfWeek.Wednesday;
                    return true;
                case "thu":
                    dayOfWeek = DayOfWeek.Thursday;
                    return true;
                case "fri":
                    dayOfWeek = DayOfWeek.Friday;
                    return true;
                case "sat":
                    dayOfWeek = DayOfWeek.Saturday;
                    return true;
            }
        }

        dayOfWeek = default;
        return false;
    }

    private static bool IsValidMonthDay(string value)
    {
        var parts = value.Split('-', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts.Length != 2)
        {
            return false;
        }

        if (!int.TryParse(parts[0], out var month) || !int.TryParse(parts[1], out var day))
        {
            return false;
        }

        if (month < 1 || month > 12)
        {
            return false;
        }

        var maxDay = DateTime.DaysInMonth(2000, month);
        return day >= 1 && day <= maxDay;
    }
}

public sealed record CalendarDefinitionValidationResult(
    string CalendarId,
    string Name,
    string? Description,
    string TimeZoneId,
    CalendarMode Mode,
    IReadOnlyCollection<CalendarRuleDefinition> Rules);
