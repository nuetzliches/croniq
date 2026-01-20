using System;
using System.Collections.Generic;

namespace Croniq.Api.Models;

public sealed record WebhookActivityTimelineEntry(
    string Id,
    string Kind,
    string Status,
    string HookKey,
    string? JobKey,
    string? Environment,
    string? Source,
    DateTimeOffset OccurredAtUtc,
    int? LatencyMs,
    int? PayloadBytes,
    string? RequestId,
    string? Reason,
    long? DeadLetterId);

public sealed record WebhookActivityBucket(
    DateTimeOffset BucketStartUtc,
    DateTimeOffset? BucketEndUtc,
    int TotalCount,
    int ErrorCount,
    int? P95LatencyMs);

public sealed record WebhookActivitySummary(
    int BucketMinutes,
    DateTimeOffset WindowStartUtc,
    DateTimeOffset WindowEndUtc,
    IReadOnlyCollection<WebhookActivityBucket> Buckets);

public sealed record WebhookActivityStreamEvent(
    string Type,
    DateTimeOffset EmittedAtUtc,
    DateTimeOffset? LatestOccurredAtUtc);
