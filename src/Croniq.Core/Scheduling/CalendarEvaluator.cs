using System;
using System.Diagnostics;
using System.Globalization;
using System.Linq;
using Croniq.Persistence.Abstractions;
using Microsoft.Extensions.Logging;

namespace Croniq.Core.Scheduling;

public static class CalendarEvaluator
{
    private static readonly string[] TimeFormats = { "hh\\:mm", "h\\:mm", "hh\\:mm\\:ss", "h\\:mm\\:ss" };

    public static DateTimeOffset? GetNextOccurrence(
        TriggerDefinition trigger,
        DateTimeOffset referenceUtc,
        CalendarDefinition? calendar,
        CalendarEvaluationOptions? options = null,
        ILogger? logger = null)
    {
        if (trigger is null) throw new ArgumentNullException(nameof(trigger));

        options ??= new CalendarEvaluationOptions();
        options.Normalize();

        var triggerTimeZone = TimeZoneUtil.ResolveTimeZone(trigger.TimeZoneId);
        if (calendar is null || !calendar.Enabled)
        {
            return TriggerSchedule.GetNextOccurrence(
                trigger.ScheduleExpression,
                referenceUtc,
                trigger.StartAtUtc,
                trigger.EndAtUtc,
                triggerTimeZone);
        }

        var stopwatch = Stopwatch.StartNew();
        var skipped = 0;
        var attempts = 0;
        var ruleHits = 0;
        var result = "none";
        var lookaheadLimit = referenceUtc.AddDays(options.MaxLookaheadDays);
        var cursor = referenceUtc;
        var calendarTimeZone = ResolveCalendarTimeZone(calendar, logger);

        SetActivityTags(calendar, calendarTimeZone, 0, 0);

        while (attempts < options.MaxCandidateIterations)
        {
            var candidate = TriggerSchedule.GetNextOccurrence(
                trigger.ScheduleExpression,
                cursor,
                trigger.StartAtUtc,
                trigger.EndAtUtc,
                triggerTimeZone);

            if (!candidate.HasValue)
            {
                result = "none";
                break;
            }

            if (candidate.Value > lookaheadLimit)
            {
                result = "limit";
                logger?.LogWarning(
                    "Calendar evaluation exceeded lookahead limit for trigger {TriggerId} and calendar {CalendarId}.",
                    trigger.TriggerId,
                    calendar.CalendarId);
                break;
            }

            if (!IsCandidateIncluded(calendar, calendarTimeZone, candidate.Value, logger, out ruleHits))
            {
                skipped += 1;

                if (TriggerSchedule.IsOnceExpression(trigger.ScheduleExpression))
                {
                    result = "excluded-once";
                    logger?.LogInformation(
                        "Calendar {CalendarId} excluded @once trigger {TriggerId}; no next occurrence.",
                        calendar.CalendarId,
                        trigger.TriggerId);
                    break;
                }

                if (candidate.Value <= cursor)
                {
                    result = "non-advancing";
                    logger?.LogWarning(
                        "Calendar evaluation detected non-advancing schedule for trigger {TriggerId}.",
                        trigger.TriggerId);
                    break;
                }

                cursor = candidate.Value;
                attempts += 1;
                continue;
            }

            result = "included";
            stopwatch.Stop();
            CalendarMetrics.RecordSkipped(calendar.Mode, skipped, trigger.Scope);
            CalendarMetrics.RecordEvaluation(calendar.Mode, result, stopwatch.Elapsed.TotalMilliseconds, trigger.Scope);
            SetActivityTags(calendar, calendarTimeZone, ruleHits, skipped);
            return candidate;
        }

        stopwatch.Stop();
        CalendarMetrics.RecordSkipped(calendar.Mode, skipped, trigger.Scope);
        CalendarMetrics.RecordEvaluation(calendar.Mode, result, stopwatch.Elapsed.TotalMilliseconds, trigger.Scope);
        SetActivityTags(calendar, calendarTimeZone, ruleHits, skipped);
        return null;
    }

    private static bool IsCandidateIncluded(
        CalendarDefinition calendar,
        TimeZoneInfo calendarTimeZone,
        DateTimeOffset candidateUtc,
        ILogger? logger,
        out int ruleHits)
    {
        ruleHits = 0;
        var candidateLocal = TimeZoneUtil.ConvertTime(candidateUtc, calendarTimeZone);
        var rules = calendar.Rules
            .Where(rule => rule.IsEnabled)
            .OrderBy(rule => rule.SortOrder);

        if (calendar.Mode == CalendarMode.Include)
        {
            foreach (var rule in rules)
            {
                if (TryMatchRule(rule, candidateUtc, candidateLocal, calendarTimeZone, logger))
                {
                    ruleHits += 1;
                    return true;
                }
            }

            return false;
        }

        foreach (var rule in rules)
        {
            if (TryMatchRule(rule, candidateUtc, candidateLocal, calendarTimeZone, logger))
            {
                ruleHits += 1;
                return false;
            }
        }

        return true;
    }

    private static bool TryMatchRule(
        CalendarRuleDefinition rule,
        DateTimeOffset candidateUtc,
        DateTimeOffset candidateLocal,
        TimeZoneInfo calendarTimeZone,
        ILogger? logger)
    {
        switch (rule.RuleType)
        {
            case CalendarRuleType.DailyWindow:
                if (rule.DailyWindow is null)
                {
                    return false;
                }

                return MatchesDailyWindow(rule.DailyWindow, candidateLocal, logger);
            case CalendarRuleType.WeeklyWindow:
                if (rule.WeeklyWindow is null)
                {
                    return false;
                }

                return MatchesWeeklyWindow(rule.WeeklyWindow, candidateLocal, logger);
            case CalendarRuleType.AnnualDateList:
                if (rule.AnnualDateList is null)
                {
                    return false;
                }

                return MatchesAnnualDateList(rule.AnnualDateList, candidateLocal, logger);
            case CalendarRuleType.DateList:
                if (rule.DateList is null)
                {
                    return false;
                }

                return MatchesDateList(rule.DateList, candidateLocal, logger);
            case CalendarRuleType.CronRule:
                if (rule.CronRule is null)
                {
                    return false;
                }

                return MatchesCronRule(rule.CronRule, candidateUtc, calendarTimeZone, logger);
            default:
                return false;
        }
    }

    private static bool MatchesDailyWindow(CalendarDailyWindowRule rule, DateTimeOffset candidateLocal, ILogger? logger)
    {
        if (!TryParseTimeOfDay(rule.StartTime, out var start))
        {
            logger?.LogWarning("Calendar daily window start time '{StartTime}' is invalid.", rule.StartTime);
            return false;
        }

        if (!TryParseTimeOfDay(rule.EndTime, out var end))
        {
            logger?.LogWarning("Calendar daily window end time '{EndTime}' is invalid.", rule.EndTime);
            return false;
        }

        if (rule.DaysOfWeek is { Count: > 0 } days)
        {
            if (!TryBuildDayOfWeekSet(days, out var allowed))
            {
                logger?.LogWarning("Calendar daily window has invalid DaysOfWeek values.");
                return false;
            }

            if (!allowed.Contains(candidateLocal.DayOfWeek))
            {
                return false;
            }
        }

        var time = candidateLocal.TimeOfDay;
        if (start == end)
        {
            return true;
        }

        if (start < end)
        {
            return time >= start && time < end;
        }

        return time >= start || time < end;
    }

    private static bool MatchesWeeklyWindow(CalendarWeeklyWindowRule rule, DateTimeOffset candidateLocal, ILogger? logger)
    {
        if (!TryBuildDayOfWeekSet(rule.DaysOfWeek, out var allowed))
        {
            logger?.LogWarning("Calendar weekly window has invalid DaysOfWeek values.");
            return false;
        }

        return allowed.Contains(candidateLocal.DayOfWeek);
    }

    private static bool MatchesAnnualDateList(CalendarAnnualDateListRule rule, DateTimeOffset candidateLocal, ILogger? logger)
    {
        foreach (var value in rule.MonthDays)
        {
            if (!TryParseMonthDay(value, out var month, out var day))
            {
                logger?.LogWarning("Calendar annual date list entry '{MonthDay}' is invalid.", value);
                continue;
            }

            if (candidateLocal.Month == month && candidateLocal.Day == day)
            {
                return true;
            }
        }

        return false;
    }

    private static bool MatchesDateList(CalendarDateListRule rule, DateTimeOffset candidateLocal, ILogger? logger)
    {
        var candidateDate = DateOnly.FromDateTime(candidateLocal.DateTime);
        foreach (var value in rule.Dates)
        {
            if (!DateOnly.TryParseExact(value, "yyyy-MM-dd", CultureInfo.InvariantCulture, DateTimeStyles.None, out var date))
            {
                logger?.LogWarning("Calendar date list entry '{Date}' is invalid.", value);
                continue;
            }

            if (date == candidateDate)
            {
                return true;
            }
        }

        return false;
    }

    private static bool MatchesCronRule(CalendarCronRule rule, DateTimeOffset candidateUtc, TimeZoneInfo timeZone, ILogger? logger)
    {
        try
        {
            var expression = new CronExpression(rule.CronExpression);
            expression.TimeZone = timeZone;
            return expression.IsSatisfiedBy(candidateUtc);
        }
        catch (Exception ex)
        {
            logger?.LogWarning(ex, "Calendar cron rule '{CronExpression}' is invalid.", rule.CronExpression);
            return false;
        }
    }

    private static bool TryParseTimeOfDay(string value, out TimeSpan timeOfDay)
    {
        return TimeSpan.TryParseExact(value, TimeFormats, CultureInfo.InvariantCulture, out timeOfDay);
    }

    private static bool TryBuildDayOfWeekSet(IReadOnlyCollection<string> values, out HashSet<DayOfWeek> days)
    {
        days = new HashSet<DayOfWeek>();
        foreach (var value in values)
        {
            if (!TryParseDayOfWeek(value, out var day))
            {
                return false;
            }

            days.Add(day);
        }

        return days.Count > 0;
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

    private static bool TryParseMonthDay(string value, out int month, out int day)
    {
        month = 0;
        day = 0;
        var parts = value.Split('-', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        if (parts.Length != 2)
        {
            return false;
        }

        if (!int.TryParse(parts[0], out month) || !int.TryParse(parts[1], out day))
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

    private static TimeZoneInfo ResolveCalendarTimeZone(CalendarDefinition calendar, ILogger? logger)
    {
        if (string.IsNullOrWhiteSpace(calendar.TimeZoneId))
        {
            return TimeZoneInfo.Utc;
        }

        if (TimeZoneUtil.TryFindTimeZoneById(calendar.TimeZoneId, out var timeZone))
        {
            return timeZone!;
        }

        logger?.LogWarning("Calendar {CalendarId} has invalid time zone '{TimeZoneId}', falling back to UTC.", calendar.CalendarId, calendar.TimeZoneId);
        return TimeZoneInfo.Utc;
    }

    private static void SetActivityTags(CalendarDefinition calendar, TimeZoneInfo timeZone, int ruleHits, int skipped)
    {
        var activity = Activity.Current;
        if (activity is null)
        {
            return;
        }

        activity.SetTag("calendar.id", calendar.CalendarId);
        activity.SetTag("calendar.mode", calendar.Mode == CalendarMode.Include ? "include" : "exclude");
        activity.SetTag("calendar.timezone", timeZone.Id);
        activity.SetTag("calendar.rule_hits", ruleHits);
        activity.SetTag("calendar.skipped_candidates", skipped);
    }
}
