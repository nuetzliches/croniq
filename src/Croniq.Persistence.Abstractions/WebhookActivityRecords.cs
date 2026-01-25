using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Croniq.Persistence.Abstractions;

public enum WebhookActivityKind
{
    Delivery = 0,
    DeadLetter = 1
}

public enum WebhookActivityStatus
{
    Success = 0,
    Failed = 1,
    Warning = 2,
    Pending = 3,
    Leased = 4
}

public static class WebhookActivitySources
{
    public const string Ingress = "ingress";
    public const string Invoke = "invoke";
}

public static class WebhookActivityMetadata
{
    public const string SourceKey = "webhook:source";
}

public static class WebhookActivityHeaders
{
    public const string SourceHeaderName = "X-Croniq-Activity-Source";
}

public sealed record WebhookActivityEntry(
    string Id,
    WebhookActivityKind Kind,
    WebhookActivityStatus Status,
    string HookKey,
    string? JobKey,
    string TenantId,
    string EnvironmentTag,
    string? Source,
    DateTimeOffset OccurredAtUtc,
    int? LatencyMs,
    int? Attempts,
    string? Reason,
    int? PayloadBytes,
    long? DeadLetterId);

public sealed record WebhookActivityBucket(
    DateTimeOffset BucketStartUtc,
    DateTimeOffset BucketEndUtc,
    int TotalCount,
    int ErrorCount,
    int WarningCount,
    int PendingCount,
    int LeasedCount,
    int DeadLetterCount,
    int? P95LatencyMs);

public sealed record WebhookActivitySummary(
    int BucketMinutes,
    DateTimeOffset WindowStartUtc,
    DateTimeOffset WindowEndUtc,
    IReadOnlyCollection<WebhookActivityBucket> Buckets);

public sealed record WebhookActivityRecord(
    string EventId,
    string HookKey,
    string JobKey,
    string TenantId,
    string EnvironmentTag,
    DateTimeOffset OccurredAtUtc,
    WebhookActivityStatus Status,
    string Source,
    string? Reason,
    string? Payload,
    IReadOnlyDictionary<string, string>? Metadata);

public sealed class WebhookActivityQuery
{
    public const int DefaultLimit = 200;
    public const int MaxLimit = 500;

    public DateTimeOffset? FromUtc { get; init; }

    public DateTimeOffset? ToUtc { get; init; }

    public DateTimeOffset? UpdatedSinceUtc { get; init; }

    public IReadOnlyCollection<string>? HookKeys { get; init; }

    public IReadOnlyCollection<string>? JobKeys { get; init; }

    public int Limit { get; init; } = DefaultLimit;

    public WebhookActivityQuery Normalize()
    {
        var limit = Limit <= 0 ? DefaultLimit : Limit;
        limit = Math.Clamp(limit, 1, MaxLimit);

        return new WebhookActivityQuery
        {
            FromUtc = FromUtc,
            ToUtc = ToUtc,
            UpdatedSinceUtc = UpdatedSinceUtc,
            HookKeys = HookKeys,
            JobKeys = JobKeys,
            Limit = limit
        };
    }
}

public sealed class WebhookActivitySummaryQuery
{
    public const int DefaultBucketMinutes = 60;
    public const int DefaultWindowMinutes = 1440;
    public const int MaxWindowMinutes = 44640;
    public const int MaxBucketMinutes = 1440;

    public DateTimeOffset? FromUtc { get; init; }

    public DateTimeOffset? ToUtc { get; init; }

    public IReadOnlyCollection<string>? HookKeys { get; init; }

    public IReadOnlyCollection<string>? JobKeys { get; init; }

    public int? BucketMinutes { get; init; }

    public WebhookActivitySummaryQuery Normalize(DateTimeOffset nowUtc)
    {
        var toUtc = ToUtc ?? nowUtc;
        var fromUtc = FromUtc ?? toUtc.AddMinutes(-DefaultWindowMinutes);
        var bucketMinutes = BucketMinutes ?? DefaultBucketMinutes;
        if (bucketMinutes <= 0)
        {
            bucketMinutes = DefaultBucketMinutes;
        }

        return new WebhookActivitySummaryQuery
        {
            FromUtc = fromUtc,
            ToUtc = toUtc,
            HookKeys = HookKeys,
            JobKeys = JobKeys,
            BucketMinutes = bucketMinutes
        };
    }
}

public interface IWebhookActivityStore
{
    Task<IReadOnlyCollection<WebhookActivityEntry>> ListAsync(
        PartitionScope scope,
        WebhookActivityQuery query,
        CancellationToken cancellationToken);

    Task<WebhookActivitySummary> SummarizeAsync(
        PartitionScope scope,
        WebhookActivitySummaryQuery query,
        CancellationToken cancellationToken);
}

public interface IWebhookActivityRecorder
{
    Task RecordAsync(WebhookActivityRecord record, CancellationToken cancellationToken);
}
