using System;
using System.Collections.Generic;

namespace Croniq.Api.Models;

public sealed record ScheduleForecastResponse(
    DateTimeOffset GeneratedAtUtc,
    DateTimeOffset WindowStartUtc,
    DateTimeOffset WindowEndUtc,
    int BucketMinutes,
    IReadOnlyList<ScheduleForecastBucket> Buckets,
    IReadOnlyList<ScheduleForecastSummary> Summaries,
    int TotalSchedules,
    int ActiveSchedules);

public sealed record ScheduleForecastBucket(
    DateTimeOffset StartAtUtc,
    DateTimeOffset EndAtUtc,
    int Count);

public sealed record ScheduleForecastSummary(
    int WindowMinutes,
    int Count);
