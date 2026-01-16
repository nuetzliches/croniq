using System;
using System.Collections.Generic;
using System.Linq;
using Croniq.Api.Models;
using Croniq.Auth.Abstractions;
using Croniq.Core.Scheduling;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Http;
using Microsoft.AspNetCore.Mvc;
using Microsoft.Extensions.Logging;

namespace Croniq.Api;

public static partial class ApiHostingExtensions
{
    private const int DefaultForecastWindowMinutes = 60;
    private const int DefaultForecastBucketMinutes = 5;
    private const int MaxForecastWindowMinutes = 240;
    private static readonly int[] DefaultForecastSummaryMinutes = { 5, 15, 60 };

    private static void MapDashboardEndpoints(WebApplication app)
    {
        app.MapGet("/tenants/{tenantId}/dashboard/forecast", async (
            string tenantId,
            string? environment,
            int? windowMinutes,
            int? bucketMinutes,
            string? summaryMinutes,
            [FromServices] ICallerContextAccessor callerContextAccessor,
            [FromServices] IJobPersistenceProvider store,
            [FromServices] ILoggerFactory loggerFactory,
            CancellationToken cancellationToken) =>
        {
            var resolvedEnvironment = ResolveEnvironmentTag(environment, callerContextAccessor);
            if (string.IsNullOrWhiteSpace(resolvedEnvironment))
            {
                return MissingEnvironment();
            }

            if (!TryNormalizeForecastQuery(windowMinutes, bucketMinutes, summaryMinutes, out var normalized, out var error))
            {
                return Results.BadRequest(new { error = "invalid-forecast-query", message = error });
            }

            var scope = new PartitionScope(tenantId, resolvedEnvironment);
            var triggers = await store.ListTriggersAsync(scope, cancellationToken).ConfigureAwait(false);
            var logger = loggerFactory.CreateLogger("Croniq.Api.DashboardForecast");
            var response = BuildScheduleForecast(triggers, normalized, logger);
            return Results.Ok(response);
        })
        .WithDocs("Dashboard_Forecast", "Get schedule forecast", "Returns an aggregated forecast for schedule executions within the requested window.")
        .Produces<ScheduleForecastResponse>(StatusCodes.Status200OK)
        .Produces(StatusCodes.Status400BadRequest)
        .RequireCroniqTenantScope(CroniqScopes.SchedulesWrite);
    }

    private static bool TryNormalizeForecastQuery(
        int? windowMinutes,
        int? bucketMinutes,
        string? summaryMinutes,
        out ForecastQuery normalized,
        out string error)
    {
        normalized = default;
        error = string.Empty;

        var window = windowMinutes ?? DefaultForecastWindowMinutes;
        if (window <= 0 || window > MaxForecastWindowMinutes)
        {
            error = $"windowMinutes must be between 1 and {MaxForecastWindowMinutes}.";
            return false;
        }

        var bucket = bucketMinutes ?? DefaultForecastBucketMinutes;
        if (bucket <= 0 || bucket > window)
        {
            error = "bucketMinutes must be greater than zero and less than or equal to windowMinutes.";
            return false;
        }

        if (window % bucket != 0)
        {
            error = "windowMinutes must be evenly divisible by bucketMinutes.";
            return false;
        }

        var summaries = NormalizeSummaryMinutes(summaryMinutes, window, bucket, out error);
        if (summaries is null)
        {
            return false;
        }

        normalized = new ForecastQuery(window, bucket, summaries);
        return true;
    }

    private static IReadOnlyList<int>? NormalizeSummaryMinutes(
        string? summaryMinutes,
        int windowMinutes,
        int bucketMinutes,
        out string error)
    {
        error = string.Empty;
        IReadOnlyList<int> raw = string.IsNullOrWhiteSpace(summaryMinutes)
            ? DefaultForecastSummaryMinutes
            : ParseSummaryMinutes(summaryMinutes);

        if (raw.Count == 0)
        {
            error = "summaryMinutes must include at least one entry.";
            return null;
        }

        var distinct = raw
            .Where(value => value > 0)
            .Distinct()
            .OrderBy(value => value)
            .ToArray();

        if (distinct.Length == 0)
        {
            error = "summaryMinutes must include positive values.";
            return null;
        }

        if (distinct.Any(value => value > windowMinutes))
        {
            error = "summaryMinutes cannot exceed windowMinutes.";
            return null;
        }

        if (distinct.Any(value => value % bucketMinutes != 0))
        {
            error = "summaryMinutes values must be evenly divisible by bucketMinutes.";
            return null;
        }

        return distinct;
    }

    private static List<int> ParseSummaryMinutes(string summaryMinutes)
    {
        var results = new List<int>();
        var tokens = summaryMinutes.Split(new[] { ',', ';', ' ' }, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
        foreach (var token in tokens)
        {
            if (int.TryParse(token, out var value))
            {
                results.Add(value);
            }
        }

        return results;
    }

    private static ScheduleForecastResponse BuildScheduleForecast(
        IReadOnlyCollection<TriggerDefinition> triggers,
        ForecastQuery query,
        ILogger logger)
    {
        var nowUtc = DateTimeOffset.UtcNow;
        var windowStartUtc = nowUtc;
        var windowEndUtc = windowStartUtc.AddMinutes(query.WindowMinutes);
        var bucketSpan = TimeSpan.FromMinutes(query.BucketMinutes);
        var bucketCount = query.WindowMinutes / query.BucketMinutes;
        var bucketCounts = new int[bucketCount];

        foreach (var trigger in triggers)
        {
            if (!trigger.Enabled)
            {
                continue;
            }

            if (string.IsNullOrWhiteSpace(trigger.ScheduleExpression))
            {
                continue;
            }

            if (trigger.EndAtUtc.HasValue && trigger.EndAtUtc.Value <= windowStartUtc)
            {
                continue;
            }

            var timeZone = ResolveTimeZone(trigger.TimeZoneId, logger);

            if (TriggerSchedule.IsOnceExpression(trigger.ScheduleExpression))
            {
                AddOnceOccurrence(trigger, windowStartUtc, windowEndUtc, bucketSpan, bucketCounts);
                continue;
            }

            AddCronOccurrences(trigger, timeZone, windowStartUtc, windowEndUtc, bucketSpan, bucketCounts, logger);
        }

        var buckets = new ScheduleForecastBucket[bucketCount];
        for (var index = 0; index < bucketCount; index++)
        {
            var start = windowStartUtc.AddMinutes(index * query.BucketMinutes);
            var end = start.Add(bucketSpan);
            buckets[index] = new ScheduleForecastBucket(start, end, bucketCounts[index]);
        }

        var summaries = query.SummaryMinutes
            .Select(summary =>
            {
                var bucketLimit = summary / query.BucketMinutes;
                var count = 0;
                for (var i = 0; i < bucketLimit && i < bucketCounts.Length; i++)
                {
                    count += bucketCounts[i];
                }

                return new ScheduleForecastSummary(summary, count);
            })
            .ToArray();

        var totalSchedules = triggers.Count;
        var activeSchedules = triggers.Count(trigger => trigger.Enabled);

        return new ScheduleForecastResponse(
            nowUtc,
            windowStartUtc,
            windowEndUtc,
            query.BucketMinutes,
            buckets,
            summaries,
            totalSchedules,
            activeSchedules);
    }

    private static void AddOnceOccurrence(
        TriggerDefinition trigger,
        DateTimeOffset windowStartUtc,
        DateTimeOffset windowEndUtc,
        TimeSpan bucketSpan,
        int[] bucketCounts)
    {
        var fireAt = trigger.StartAtUtc ?? windowStartUtc;
        if (fireAt < windowStartUtc)
        {
            fireAt = windowStartUtc;
        }

        if (trigger.EndAtUtc.HasValue && fireAt > trigger.EndAtUtc.Value)
        {
            return;
        }

        if (fireAt >= windowEndUtc)
        {
            return;
        }

        AddOccurrence(fireAt, windowStartUtc, bucketSpan, bucketCounts);
    }

    private static void AddCronOccurrences(
        TriggerDefinition trigger,
        TimeZoneInfo timeZone,
        DateTimeOffset windowStartUtc,
        DateTimeOffset windowEndUtc,
        TimeSpan bucketSpan,
        int[] bucketCounts,
        ILogger logger)
    {
        var cursor = windowStartUtc;
        if (trigger.StartAtUtc.HasValue && trigger.StartAtUtc.Value > cursor)
        {
            cursor = trigger.StartAtUtc.Value;
        }

        if (trigger.EndAtUtc.HasValue && cursor > trigger.EndAtUtc.Value)
        {
            return;
        }

        var maxOccurrences = (int)Math.Ceiling((windowEndUtc - cursor).TotalSeconds) + 2;
        var count = 0;
        var next = TriggerSchedule.GetNextOccurrence(
            trigger.ScheduleExpression,
            cursor,
            trigger.StartAtUtc,
            trigger.EndAtUtc,
            timeZone);

        while (next.HasValue && next.Value < windowEndUtc && count < maxOccurrences)
        {
            AddOccurrence(next.Value, windowStartUtc, bucketSpan, bucketCounts);
            count += 1;

            var previous = next.Value;
            next = TriggerSchedule.GetNextOccurrence(
                trigger.ScheduleExpression,
                previous,
                trigger.StartAtUtc,
                trigger.EndAtUtc,
                timeZone);

            if (next.HasValue && next.Value <= previous)
            {
                logger.LogWarning(
                    "Dashboard forecast detected non-advancing schedule '{TriggerId}' ({Expression}).",
                    trigger.TriggerId,
                    trigger.ScheduleExpression);
                break;
            }
        }

        if (count >= maxOccurrences)
        {
            logger.LogWarning(
                "Dashboard forecast reached the max occurrence limit for '{TriggerId}'.",
                trigger.TriggerId);
        }
    }

    private static void AddOccurrence(
        DateTimeOffset occurrenceUtc,
        DateTimeOffset windowStartUtc,
        TimeSpan bucketSpan,
        int[] bucketCounts)
    {
        var offset = occurrenceUtc - windowStartUtc;
        if (offset < TimeSpan.Zero)
        {
            return;
        }

        var index = (int)(offset.Ticks / bucketSpan.Ticks);
        if (index < 0 || index >= bucketCounts.Length)
        {
            return;
        }

        bucketCounts[index] += 1;
    }

    private static TimeZoneInfo ResolveTimeZone(string? timeZoneId, ILogger logger)
    {
        if (string.IsNullOrWhiteSpace(timeZoneId))
        {
            return TimeZoneInfo.Utc;
        }

        try
        {
            return TimeZoneInfo.FindSystemTimeZoneById(timeZoneId);
        }
        catch (TimeZoneNotFoundException ex)
        {
            logger.LogWarning(ex, "Dashboard forecast falling back to UTC for invalid timezone '{TimeZoneId}'.", timeZoneId);
            return TimeZoneInfo.Utc;
        }
        catch (InvalidTimeZoneException ex)
        {
            logger.LogWarning(ex, "Dashboard forecast falling back to UTC for invalid timezone '{TimeZoneId}'.", timeZoneId);
            return TimeZoneInfo.Utc;
        }
    }

    private readonly record struct ForecastQuery(
        int WindowMinutes,
        int BucketMinutes,
        IReadOnlyList<int> SummaryMinutes);
}
