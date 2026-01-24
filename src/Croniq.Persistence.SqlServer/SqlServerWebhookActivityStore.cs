using System;
using System.Collections.Generic;
using System.Linq;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Data.SqlServer;
using Croniq.Data.SqlServer.Entities;
using Croniq.Persistence.Abstractions;
using Microsoft.EntityFrameworkCore;

namespace Croniq.Persistence.SqlServer;

public sealed class SqlServerWebhookActivityStore : IWebhookActivityStore, IWebhookActivityRecorder
{
    private const string StatusPending = "Pending";
    private const string StatusLeased = "Leased";
    private const string StatusDelivered = "Delivered";
    private const string StatusFailed = "Failed";
    private const int ErrorMaxLength = 1024;
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);
    private readonly IDbContextFactory<SqlServerDbContext> _dbFactory;

    public SqlServerWebhookActivityStore(IDbContextFactory<SqlServerDbContext> dbFactory)
    {
        _dbFactory = dbFactory ?? throw new ArgumentNullException(nameof(dbFactory));
    }

    public async Task<IReadOnlyCollection<WebhookActivityEntry>> ListAsync(
        PartitionScope scope,
        WebhookActivityQuery query,
        CancellationToken cancellationToken)
    {
        if (query is null) throw new ArgumentNullException(nameof(query));

        var normalized = query.Normalize();
        if (normalized.Limit <= 0)
        {
            return Array.Empty<WebhookActivityEntry>();
        }

        var hookKeys = NormalizeKeys(normalized.HookKeys);
        var jobKeys = NormalizeKeys(normalized.JobKeys);
        var fromUtc = normalized.FromUtc?.UtcDateTime;
        var toUtc = normalized.ToUtc?.UtcDateTime;
        var updatedSinceUtc = normalized.UpdatedSinceUtc?.UtcDateTime;

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        var ingress = await BuildIngressQuery(db, scope, hookKeys, jobKeys, fromUtc, toUtc, updatedSinceUtc)
            .OrderByDescending(entry => entry.ReceivedAtUtc)
            .ThenByDescending(entry => entry.Id)
            .Take(normalized.Limit)
            .Select(entry => new IngressSnapshot(
                entry.EventId,
                entry.HookKey,
                entry.JobKey,
                entry.TenantId,
                entry.EnvironmentTag,
                entry.ReceivedAtUtc,
                entry.UpdatedAtUtc,
                entry.Status,
                entry.AttemptCount,
                entry.LastError,
                entry.Payload,
                entry.MetadataJson))
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        var deadLetters = await BuildDeadLetterQuery(db, scope, hookKeys, jobKeys, fromUtc, toUtc, updatedSinceUtc)
            .OrderByDescending(entry => entry.CreatedAtUtc)
            .ThenByDescending(entry => entry.Id)
            .Take(normalized.Limit)
            .Select(entry => new DeadLetterSnapshot(
                entry.Id,
                entry.HookKey,
                entry.JobKey,
                entry.TenantId,
                entry.EnvironmentTag,
                entry.CreatedAtUtc,
                entry.FailureReason,
                entry.ErrorDetails,
                entry.Payload))
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        if (ingress.Count == 0 && deadLetters.Count == 0)
        {
            return Array.Empty<WebhookActivityEntry>();
        }

        var entries = new List<WebhookActivityEntry>(ingress.Count + deadLetters.Count);
        entries.AddRange(ingress.Select(MapIngress));
        entries.AddRange(deadLetters.Select(MapDeadLetter));

        return entries
            .OrderByDescending(entry => entry.OccurredAtUtc)
            .ThenByDescending(entry => entry.Kind)
            .Take(normalized.Limit)
            .ToArray();
    }

    public async Task<WebhookActivitySummary> SummarizeAsync(
        PartitionScope scope,
        WebhookActivitySummaryQuery query,
        CancellationToken cancellationToken)
    {
        if (query is null) throw new ArgumentNullException(nameof(query));

        var nowUtc = DateTimeOffset.UtcNow;
        var normalized = query.Normalize(nowUtc);
        var bucketMinutes = Math.Clamp(
            normalized.BucketMinutes ?? WebhookActivitySummaryQuery.DefaultBucketMinutes,
            1,
            WebhookActivitySummaryQuery.MaxBucketMinutes);
        var windowStartUtc = normalized.FromUtc ?? nowUtc.AddMinutes(-WebhookActivitySummaryQuery.DefaultWindowMinutes);
        var windowEndUtc = normalized.ToUtc ?? nowUtc;

        var hookKeys = NormalizeKeys(normalized.HookKeys);
        var jobKeys = NormalizeKeys(normalized.JobKeys);

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);

        var ingressSamples = await BuildIngressQuery(
                db,
                scope,
                hookKeys,
                jobKeys,
                windowStartUtc.UtcDateTime,
                windowEndUtc.UtcDateTime,
                updatedSinceUtc: null)
            .Select(entry => new ActivitySample(
                new DateTimeOffset(DateTime.SpecifyKind(entry.ReceivedAtUtc, DateTimeKind.Utc)),
                ResolveIngressStatus(entry.Status, entry.AttemptCount),
                WebhookActivityKind.Delivery))
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        var deadLetterSamples = await BuildDeadLetterQuery(
                db,
                scope,
                hookKeys,
                jobKeys,
                windowStartUtc.UtcDateTime,
                windowEndUtc.UtcDateTime,
                updatedSinceUtc: null)
            .Select(entry => new ActivitySample(
                new DateTimeOffset(DateTime.SpecifyKind(entry.CreatedAtUtc, DateTimeKind.Utc)),
                WebhookActivityStatus.Failed,
                WebhookActivityKind.DeadLetter))
            .ToListAsync(cancellationToken)
            .ConfigureAwait(false);

        var combined = ingressSamples.Count == 0 && deadLetterSamples.Count == 0
            ? Array.Empty<ActivitySample>()
            : ingressSamples.Concat(deadLetterSamples);

        var buckets = BuildBuckets(combined, windowStartUtc, windowEndUtc, bucketMinutes);
        return new WebhookActivitySummary(bucketMinutes, windowStartUtc, windowEndUtc, buckets);
    }

    public async Task RecordAsync(WebhookActivityRecord record, CancellationToken cancellationToken)
    {
        if (record is null) throw new ArgumentNullException(nameof(record));
        if (string.IsNullOrWhiteSpace(record.EventId)) throw new ArgumentNullException(nameof(record.EventId));
        if (string.IsNullOrWhiteSpace(record.HookKey)) throw new ArgumentNullException(nameof(record.HookKey));
        if (string.IsNullOrWhiteSpace(record.JobKey)) throw new ArgumentNullException(nameof(record.JobKey));
        if (string.IsNullOrWhiteSpace(record.TenantId)) throw new ArgumentNullException(nameof(record.TenantId));
        if (string.IsNullOrWhiteSpace(record.EnvironmentTag)) throw new ArgumentNullException(nameof(record.EnvironmentTag));
        if (string.IsNullOrWhiteSpace(record.Source)) throw new ArgumentNullException(nameof(record.Source));

        var nowUtc = DateTime.UtcNow;
        var metadata = MergeSourceMetadata(record.Metadata, record.Source);

        var entity = new WebhookIngressEventEntity
        {
            EventId = record.EventId,
            HookKey = record.HookKey,
            JobKey = record.JobKey,
            TenantId = record.TenantId,
            EnvironmentTag = record.EnvironmentTag,
            Payload = record.Payload ?? string.Empty,
            HeadersJson = null,
            MetadataJson = SerializeDictionary(metadata),
            ReceivedAtUtc = record.OccurredAtUtc.UtcDateTime,
            Status = record.Status switch
            {
                WebhookActivityStatus.Pending => StatusPending,
                WebhookActivityStatus.Leased => StatusLeased,
                WebhookActivityStatus.Failed => StatusFailed,
                _ => StatusDelivered
            },
            AttemptCount = record.Status == WebhookActivityStatus.Pending ? 0 : 1,
            LastError = record.Status == WebhookActivityStatus.Failed ? TruncateError(record.Reason) : null,
            CreatedAtUtc = nowUtc,
            UpdatedAtUtc = nowUtc
        };

        await using var db = await _dbFactory.CreateDbContextAsync(cancellationToken).ConfigureAwait(false);
        db.WebhookIngressEvents.Add(entity);

        try
        {
            await db.SaveChangesAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (DbUpdateException)
        {
            var exists = await db.WebhookIngressEvents
                .AsNoTracking()
                .AnyAsync(x => x.EventId == record.EventId, cancellationToken)
                .ConfigureAwait(false);
            if (!exists)
            {
                throw;
            }
        }
    }

    private static IQueryable<WebhookIngressEventEntity> BuildIngressQuery(
        SqlServerDbContext db,
        PartitionScope scope,
        IReadOnlyCollection<string>? hookKeys,
        IReadOnlyCollection<string>? jobKeys,
        DateTime? fromUtc,
        DateTime? toUtc,
        DateTime? updatedSinceUtc)
    {
        var query = db.WebhookIngressEvents
            .AsNoTracking()
            .Where(entry => entry.TenantId == scope.TenantId && entry.EnvironmentTag == scope.EnvironmentTag)
            .Where(entry => entry.Status == StatusPending
                            || entry.Status == StatusLeased
                            || entry.Status == StatusDelivered
                            || entry.Status == StatusFailed);

        if (fromUtc.HasValue)
        {
            query = query.Where(entry => entry.ReceivedAtUtc >= fromUtc.Value);
        }

        if (toUtc.HasValue)
        {
            query = query.Where(entry => entry.ReceivedAtUtc <= toUtc.Value);
        }

        if (updatedSinceUtc.HasValue)
        {
            query = query.Where(entry => entry.UpdatedAtUtc >= updatedSinceUtc.Value);
        }

        if (hookKeys is { Count: > 0 })
        {
            query = query.Where(entry => hookKeys.Contains(entry.HookKey));
        }

        if (jobKeys is { Count: > 0 })
        {
            query = query.Where(entry => jobKeys.Contains(entry.JobKey));
        }

        return query;
    }

    private static IQueryable<WebhookDeadLetterEntity> BuildDeadLetterQuery(
        SqlServerDbContext db,
        PartitionScope scope,
        IReadOnlyCollection<string>? hookKeys,
        IReadOnlyCollection<string>? jobKeys,
        DateTime? fromUtc,
        DateTime? toUtc,
        DateTime? updatedSinceUtc)
    {
        var query = db.WebhookDeadLetters
            .AsNoTracking()
            .Where(entry => entry.TenantId == scope.TenantId && entry.EnvironmentTag == scope.EnvironmentTag);

        if (fromUtc.HasValue)
        {
            query = query.Where(entry => entry.CreatedAtUtc >= fromUtc.Value);
        }

        if (toUtc.HasValue)
        {
            query = query.Where(entry => entry.CreatedAtUtc <= toUtc.Value);
        }

        if (updatedSinceUtc.HasValue)
        {
            query = query.Where(entry => entry.CreatedAtUtc >= updatedSinceUtc.Value);
        }

        if (hookKeys is { Count: > 0 })
        {
            query = query.Where(entry => hookKeys.Contains(entry.HookKey));
        }

        if (jobKeys is { Count: > 0 })
        {
            query = query.Where(entry => jobKeys.Contains(entry.JobKey));
        }

        return query;
    }

    private static IReadOnlyCollection<string>? NormalizeKeys(IReadOnlyCollection<string>? values)
    {
        if (values is null || values.Count == 0)
        {
            return null;
        }

        var normalized = values
            .Where(value => !string.IsNullOrWhiteSpace(value))
            .Select(value => value.Trim())
            .Distinct(StringComparer.OrdinalIgnoreCase)
            .ToArray();

        return normalized.Length == 0 ? null : normalized;
    }

    private static WebhookActivityEntry MapIngress(IngressSnapshot entry)
    {
        var status = ResolveIngressStatus(entry.Status, entry.AttemptCount);
        var source = ResolveSource(entry.MetadataJson);
        var latencyMs = ResolveLatencyMs(entry.ReceivedAtUtc, entry.UpdatedAtUtc, status);

        return new WebhookActivityEntry(
            entry.EventId,
            WebhookActivityKind.Delivery,
            status,
            entry.HookKey,
            entry.JobKey,
            entry.TenantId,
            entry.EnvironmentTag,
            source,
            new DateTimeOffset(DateTime.SpecifyKind(entry.ReceivedAtUtc, DateTimeKind.Utc)),
            latencyMs,
            status == WebhookActivityStatus.Failed ? entry.LastError : null,
            ComputePayloadBytes(entry.Payload),
            DeadLetterId: null);
    }

    private static WebhookActivityEntry MapDeadLetter(DeadLetterSnapshot entry)
    {
        var reason = string.IsNullOrWhiteSpace(entry.ErrorDetails)
            ? entry.FailureReason
            : entry.ErrorDetails;

        return new WebhookActivityEntry(
            entry.Id.ToString(),
            WebhookActivityKind.DeadLetter,
            WebhookActivityStatus.Failed,
            entry.HookKey,
            entry.JobKey,
            entry.TenantId,
            entry.EnvironmentTag,
            WebhookActivitySources.Ingress,
            new DateTimeOffset(DateTime.SpecifyKind(entry.CreatedAtUtc, DateTimeKind.Utc)),
                LatencyMs: null,
            reason,
            ComputePayloadBytes(entry.Payload),
            entry.Id);
    }

    private static WebhookActivityStatus ResolveIngressStatus(string status, int attemptCount)
    {
        if (string.Equals(status, StatusPending, StringComparison.OrdinalIgnoreCase))
        {
            return WebhookActivityStatus.Pending;
        }

        if (string.Equals(status, StatusLeased, StringComparison.OrdinalIgnoreCase))
        {
            return WebhookActivityStatus.Leased;
        }

        if (string.Equals(status, StatusFailed, StringComparison.OrdinalIgnoreCase))
        {
            return WebhookActivityStatus.Failed;
        }

        if (string.Equals(status, StatusDelivered, StringComparison.OrdinalIgnoreCase) && attemptCount > 1)
        {
            return WebhookActivityStatus.Warning;
        }

        if (string.Equals(status, StatusDelivered, StringComparison.OrdinalIgnoreCase))
        {
            return WebhookActivityStatus.Success;
        }

        return WebhookActivityStatus.Pending;
    }

    private static int? ResolveLatencyMs(DateTime receivedAtUtc, DateTime updatedAtUtc, WebhookActivityStatus status)
    {
        if (status is WebhookActivityStatus.Pending or WebhookActivityStatus.Leased)
        {
            return null;
        }

        var delta = updatedAtUtc - receivedAtUtc;
        if (delta <= TimeSpan.Zero)
        {
            return null;
        }

        var rounded = (long)Math.Round(delta.TotalMilliseconds, MidpointRounding.AwayFromZero);
        if (rounded <= 0)
        {
            return 0;
        }

        return rounded > int.MaxValue ? int.MaxValue : (int)rounded;
    }

    private static int? ComputePayloadBytes(string? payload)
    {
        if (string.IsNullOrEmpty(payload))
        {
            return null;
        }

        return Encoding.UTF8.GetByteCount(payload);
    }

    private static string ResolveSource(string? metadataJson)
    {
        if (string.IsNullOrWhiteSpace(metadataJson))
        {
            return WebhookActivitySources.Ingress;
        }

        try
        {
            var metadata = JsonSerializer.Deserialize<Dictionary<string, string>>(metadataJson, JsonOptions);
            if (metadata is not null
                && metadata.TryGetValue(WebhookActivityMetadata.SourceKey, out var value)
                && !string.IsNullOrWhiteSpace(value))
            {
                return value;
            }
        }
        catch (JsonException)
        {
            // ignore malformed metadata
        }

        return WebhookActivitySources.Ingress;
    }

    private static IReadOnlyDictionary<string, string> MergeSourceMetadata(
        IReadOnlyDictionary<string, string>? metadata,
        string source)
    {
        var merged = metadata is null
            ? new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
            : new Dictionary<string, string>(metadata, StringComparer.OrdinalIgnoreCase);

        merged[WebhookActivityMetadata.SourceKey] = source;
        return merged;
    }

    private static string? SerializeDictionary(IReadOnlyDictionary<string, string>? values)
    {
        if (values is null || values.Count == 0)
        {
            return null;
        }

        return JsonSerializer.Serialize(values, JsonOptions);
    }

    private static string? TruncateError(string? error)
    {
        if (string.IsNullOrWhiteSpace(error))
        {
            return null;
        }

        return error.Length <= ErrorMaxLength ? error : error[..ErrorMaxLength];
    }

    private static IReadOnlyCollection<WebhookActivityBucket> BuildBuckets(
        IEnumerable<ActivitySample> samples,
        DateTimeOffset windowStartUtc,
        DateTimeOffset windowEndUtc,
        int bucketMinutes)
    {
        if (windowEndUtc <= windowStartUtc)
        {
            return Array.Empty<WebhookActivityBucket>();
        }

        var bucketSpan = TimeSpan.FromMinutes(bucketMinutes);
        if (bucketSpan <= TimeSpan.Zero)
        {
            return Array.Empty<WebhookActivityBucket>();
        }

        var bucketTicks = bucketSpan.Ticks;
        var alignedStart = AlignToBucketStart(windowStartUtc, bucketTicks);
        var alignedEnd = AlignToBucketEnd(windowEndUtc, bucketTicks);
        if (alignedEnd <= alignedStart)
        {
            alignedEnd = alignedStart.Add(bucketSpan);
        }

        var bucketCount = (int)Math.Ceiling((alignedEnd - alignedStart).TotalMinutes / bucketMinutes);
        if (bucketCount <= 0)
        {
            return Array.Empty<WebhookActivityBucket>();
        }

        var buckets = new WebhookActivityBucket[bucketCount];
        for (var index = 0; index < bucketCount; index++)
        {
            var start = alignedStart.AddMinutes(index * bucketMinutes);
            buckets[index] = new WebhookActivityBucket(
                start,
                start.Add(bucketSpan),
                TotalCount: 0,
                ErrorCount: 0,
                WarningCount: 0,
                PendingCount: 0,
                LeasedCount: 0,
                DeadLetterCount: 0,
                P95LatencyMs: null);
        }

        foreach (var sample in samples)
        {
            if (sample.OccurredAtUtc < alignedStart || sample.OccurredAtUtc >= alignedEnd)
            {
                continue;
            }

            var offsetTicks = sample.OccurredAtUtc.UtcTicks - alignedStart.UtcTicks;
            var bucketIndex = (int)(offsetTicks / bucketTicks);
            if (bucketIndex < 0 || bucketIndex >= buckets.Length)
            {
                continue;
            }

            var bucket = buckets[bucketIndex];
            bucket = bucket with
            {
                TotalCount = bucket.TotalCount + 1,
                ErrorCount = bucket.ErrorCount + (sample.Status == WebhookActivityStatus.Failed ? 1 : 0),
                WarningCount = bucket.WarningCount + (sample.Status == WebhookActivityStatus.Warning ? 1 : 0),
                PendingCount = bucket.PendingCount + (sample.Status == WebhookActivityStatus.Pending ? 1 : 0),
                LeasedCount = bucket.LeasedCount + (sample.Status == WebhookActivityStatus.Leased ? 1 : 0),
                DeadLetterCount = bucket.DeadLetterCount + (sample.Kind == WebhookActivityKind.DeadLetter ? 1 : 0)
            };
            buckets[bucketIndex] = bucket;
        }

        return buckets;
    }

    private static DateTimeOffset AlignToBucketStart(DateTimeOffset timestamp, long bucketTicks)
    {
        var ticks = timestamp.UtcTicks;
        var aligned = ticks - (ticks % bucketTicks);
        return new DateTimeOffset(aligned, TimeSpan.Zero);
    }

    private static DateTimeOffset AlignToBucketEnd(DateTimeOffset timestamp, long bucketTicks)
    {
        var ticks = timestamp.UtcTicks;
        var remainder = ticks % bucketTicks;
        var aligned = remainder == 0 ? ticks : ticks + (bucketTicks - remainder);
        return new DateTimeOffset(aligned, TimeSpan.Zero);
    }

    private sealed record IngressSnapshot(
        string EventId,
        string HookKey,
        string JobKey,
        string TenantId,
        string EnvironmentTag,
        DateTime ReceivedAtUtc,
        DateTime UpdatedAtUtc,
        string Status,
        int AttemptCount,
        string? LastError,
        string? Payload,
        string? MetadataJson);

    private sealed record DeadLetterSnapshot(
        long Id,
        string HookKey,
        string JobKey,
        string TenantId,
        string EnvironmentTag,
        DateTime CreatedAtUtc,
        string FailureReason,
        string? ErrorDetails,
        string? Payload);

    private sealed record ActivitySample(DateTimeOffset OccurredAtUtc, WebhookActivityStatus Status, WebhookActivityKind Kind);
}
