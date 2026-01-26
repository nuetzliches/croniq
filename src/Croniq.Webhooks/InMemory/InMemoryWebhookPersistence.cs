using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;
using Microsoft.AspNetCore.Http;

namespace Croniq.Webhooks.InMemory;

/// <summary>
/// Simple in-memory implementation of the webhook persistence provider for samples and tests.
/// </summary>
public sealed class InMemoryWebhookPersistenceProvider : IWebhookPersistenceProvider
{
    private readonly ConcurrentDictionary<string, WebhookEndpointDefinition> _store = new(StringComparer.OrdinalIgnoreCase);
    private long _ipRuleIdentity;
    private readonly object _ipRuleLock = new();

    private static string BuildKey(string hookKey, PartitionScope scope)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));
        return $"{scope.TenantId}:{scope.EnvironmentTag}:{hookKey}";
    }

    public WebhookEndpointDefinition? Find(string hookKey, PartitionScope scope)
    {
        _ = hookKey ?? throw new ArgumentNullException(nameof(hookKey));
        return _store.TryGetValue(BuildKey(hookKey, scope), out var definition) ? definition : null;
    }

    public WebhookEndpointDefinition Seed(
        string hookKey,
        string jobKey,
        PartitionScope scope,
        string secret,
        bool requireSignature = true,
        bool enabled = true,
        int requestsPerMinute = 120,
        IReadOnlyDictionary<string, string>? metadata = null,
        int signatureVersion = 1)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));
        if (string.IsNullOrWhiteSpace(jobKey)) throw new ArgumentNullException(nameof(jobKey));

        var now = DateTimeOffset.UtcNow;
        var materializedMetadata = metadata is null ? null : new Dictionary<string, string>(metadata, StringComparer.OrdinalIgnoreCase);

        var definition = new WebhookEndpointDefinition(
            hookKey,
            jobKey,
            secret,
            enabled,
            requireSignature,
            requestsPerMinute,
            scope.TenantId,
            scope.EnvironmentTag,
            materializedMetadata,
            Array.Empty<WebhookIpRuleDefinition>(),
            signatureVersion,
            now,
            now);

        _store[BuildKey(hookKey, scope)] = definition;
        return definition;
    }

    public Task<WebhookEndpointDefinition?> FindByHookKeyAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        return Task.FromResult(Find(hookKey, scope));
    }

    public Task<IReadOnlyCollection<WebhookEndpointDefinition>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        IReadOnlyCollection<WebhookEndpointDefinition> result = _store.Values
            .Where(def => MatchesScope(def, scope))
            .OrderBy(def => def.HookKey, StringComparer.OrdinalIgnoreCase)
            .ToArray();
        return Task.FromResult(result);
    }

    public Task UpsertAsync(WebhookEndpointUpsert request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        var now = DateTimeOffset.UtcNow;
        var metadata = request.Metadata is null ? null : new Dictionary<string, string>(request.Metadata, StringComparer.OrdinalIgnoreCase);

        var scope = new PartitionScope(request.TenantId, request.EnvironmentTag);
        _store.AddOrUpdate(
            BuildKey(request.HookKey, scope),
            _ => new WebhookEndpointDefinition(
                request.HookKey,
                request.JobKey,
                request.Secret ?? GenerateSecret(),
                request.Enabled,
                request.RequireSignature,
                request.RequestsPerMinute,
                request.TenantId,
                request.EnvironmentTag,
                metadata,
                Array.Empty<WebhookIpRuleDefinition>(),
                request.SignatureVersion,
                now,
                now),
            (_, current) => current with
            {
                JobKey = request.JobKey,
                Enabled = request.Enabled,
                RequireSignature = request.RequireSignature,
                RequestsPerMinute = request.RequestsPerMinute,
                TenantId = request.TenantId,
                EnvironmentTag = request.EnvironmentTag,
                Secret = request.Secret ?? current.Secret,
                Metadata = metadata,
                SignatureVersion = request.SignatureVersion,
                UpdatedAtUtc = now
            });

        return Task.CompletedTask;
    }

    public Task DeleteAsync(string hookKey, PartitionScope scope, bool hardDelete, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));
        _store.TryRemove(BuildKey(hookKey, scope), out _);
        return Task.CompletedTask;
    }

    public Task<WebhookSecretRotationResult> RotateSecretAsync(WebhookSecretRotate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));

        var scope = new PartitionScope(request.TenantId, request.EnvironmentTag);
        if (!_store.TryGetValue(BuildKey(request.HookKey, scope), out var current))
        {
            throw new InvalidOperationException($"webhook {request.HookKey} not found");
        }

        var nowOffset = DateTimeOffset.UtcNow;
        var secret = GenerateSecret();
        var updated = current with { Secret = secret, UpdatedAtUtc = nowOffset };
        _store[BuildKey(request.HookKey, scope)] = updated;

        var activated = DateTime.UtcNow;
        var result = new WebhookSecretRotationResult(
            request.HookKey,
            secret,
            ComputeHash(secret),
            activated,
            request.GracePeriodSeconds.HasValue ? activated.AddSeconds(request.GracePeriodSeconds.Value) : null);
        return Task.FromResult(result);
    }

    public Task<IReadOnlyCollection<WebhookSecretMaterial>> GetActiveSecretsAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));
        if (!_store.TryGetValue(BuildKey(hookKey, scope), out var definition))
        {
            return Task.FromResult<IReadOnlyCollection<WebhookSecretMaterial>>(Array.Empty<WebhookSecretMaterial>());
        }

        IReadOnlyCollection<WebhookSecretMaterial> secrets = new[]
        {
            new WebhookSecretMaterial(
                definition.Secret,
                ComputeHash(definition.Secret),
                DateTime.UtcNow,
                null)
        };

        return Task.FromResult(secrets);
    }

    public Task<IReadOnlyCollection<WebhookIpRuleDefinition>> ListIpRulesAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(hookKey)) throw new ArgumentNullException(nameof(hookKey));

        if (!_store.TryGetValue(BuildKey(hookKey, scope), out var definition))
        {
            return Task.FromResult<IReadOnlyCollection<WebhookIpRuleDefinition>>(Array.Empty<WebhookIpRuleDefinition>());
        }

        if (!MatchesScope(definition, scope))
        {
            throw new InvalidOperationException("Webhook scope mismatch.");
        }

        return Task.FromResult(definition.IpRules);
    }

    public Task<WebhookIpRuleDefinition> AddIpRuleAsync(WebhookIpRuleCreate request, CancellationToken cancellationToken)
    {
        if (request is null) throw new ArgumentNullException(nameof(request));
        var scope = new PartitionScope(request.TenantId, request.EnvironmentTag);
        if (!_store.TryGetValue(BuildKey(request.HookKey, scope), out var definition))
        {
            throw new InvalidOperationException($"webhook {request.HookKey} not found");
        }

        if (!string.Equals(definition.TenantId, request.TenantId, StringComparison.OrdinalIgnoreCase)
            || !string.Equals(definition.EnvironmentTag, request.EnvironmentTag, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException("Webhook scope mismatch.");
        }

        lock (_ipRuleLock)
        {
            if (definition.IpRules.Any(rule => string.Equals(rule.Cidr, request.Cidr, StringComparison.OrdinalIgnoreCase)))
            {
                throw new InvalidOperationException($"CIDR {request.Cidr} already exists for webhook {request.HookKey}");
            }

            var id = Interlocked.Increment(ref _ipRuleIdentity);
            var now = DateTimeOffset.UtcNow;
            var rule = new WebhookIpRuleDefinition(
                id,
                request.HookKey,
                request.TenantId,
                request.EnvironmentTag,
                request.Cidr,
                request.Description,
                request.CreatedBy,
                now,
                now);

            var updated = definition.IpRules
                .Concat(new[] { rule })
                .OrderBy(r => r.Cidr, StringComparer.OrdinalIgnoreCase)
                .ToArray();

            _store[BuildKey(request.HookKey, scope)] = definition with { IpRules = updated };
            return Task.FromResult(rule);
        }
    }

    public Task DeleteIpRuleAsync(long ruleId, PartitionScope scope, string? deletedBy, string? correlationId, CancellationToken cancellationToken)
    {
        lock (_ipRuleLock)
        {
            foreach (var kvp in _store)
            {
                var definition = kvp.Value;
                if (!MatchesScope(definition, scope))
                {
                    continue;
                }

                if (!definition.IpRules.Any(rule => rule.Id == ruleId))
                {
                    continue;
                }

                var updated = definition.IpRules
                    .Where(rule => rule.Id != ruleId)
                    .ToArray();
                _store[kvp.Key] = definition with { IpRules = updated };
                break;
            }
        }

        return Task.CompletedTask;
    }

    public void Clear()
    {
        _store.Clear();
        Interlocked.Exchange(ref _ipRuleIdentity, 0);
    }

    private static bool MatchesScope(WebhookEndpointDefinition definition, PartitionScope scope)
    {
        return string.Equals(definition.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(definition.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }

    private static string GenerateSecret() => $"whsec_{Guid.NewGuid():N}";

    private static string ComputeHash(string secret)
    {
        var bytes = Encoding.UTF8.GetBytes(secret);
        var hash = System.Security.Cryptography.SHA256.HashData(bytes);
        return Convert.ToHexString(hash);
    }
}

/// <summary>
/// In-memory dead-letter store used for development scenarios.
/// </summary>
public sealed class InMemoryWebhookDeadLetterStore : IWebhookDeadLetterStore
{
    private readonly ConcurrentDictionary<long, WebhookDeadLetterEntry> _entries = new();
    private long _identity;

    public WebhookDeadLetterEntry Seed(
        string hookKey,
        string jobKey,
        PartitionScope scope,
        string payload,
        string failureReason = "failed",
        IReadOnlyDictionary<string, string>? metadata = null)
    {
        var id = Interlocked.Increment(ref _identity);
        var now = DateTimeOffset.UtcNow;
        var entry = new WebhookDeadLetterEntry(
            id,
            hookKey,
            jobKey,
            scope.TenantId,
            scope.EnvironmentTag,
            payload,
            null,
            metadata,
            failureReason,
            Attempts: 1,
            StatusCode: StatusCodes.Status500InternalServerError,
            ErrorDetails: "seeded",
            CreatedAtUtc: now,
            LastAttemptAtUtc: now,
            NextAttemptAtUtc: null,
            ExpiresAtUtc: now.AddDays(7));

        _entries[id] = entry;
        return entry;
    }

    public Task<long> CreateAsync(WebhookDeadLetterCreate request, CancellationToken cancellationToken)
    {
        var id = Interlocked.Increment(ref _identity);
        var now = DateTimeOffset.UtcNow;
        var entry = new WebhookDeadLetterEntry(
            id,
            request.HookKey,
            request.JobKey,
            request.TenantId,
            request.EnvironmentTag,
            request.Payload,
            request.Headers,
            request.Metadata,
            request.FailureReason,
            Attempts: 1,
            StatusCode: request.StatusCode,
            ErrorDetails: request.ErrorDetails,
            CreatedAtUtc: now,
            LastAttemptAtUtc: now,
            NextAttemptAtUtc: null,
            ExpiresAtUtc: request.ExpiresAtUtc);
        _entries[id] = entry;
        return Task.FromResult(id);
    }

    public Task<IReadOnlyCollection<WebhookDeadLetterEntry>> ListAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        IReadOnlyCollection<WebhookDeadLetterEntry> result = _entries.Values
            .Where(entry => MatchesScope(entry, scope))
            .OrderBy(entry => entry.Id)
            .ToArray();
        return Task.FromResult(result);
    }

    public Task<WebhookDeadLetterEntry?> FindAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (_entries.TryGetValue(id, out var entry) && MatchesScope(entry, scope))
        {
            return Task.FromResult<WebhookDeadLetterEntry?>(entry);
        }

        return Task.FromResult<WebhookDeadLetterEntry?>(null);
    }

    public Task ResolveAsync(long id, PartitionScope scope, CancellationToken cancellationToken)
    {
        if (_entries.TryGetValue(id, out var entry) && MatchesScope(entry, scope))
        {
            _entries.TryRemove(id, out _);
        }

        return Task.CompletedTask;
    }

    public Task RecordFailureAsync(long id, PartitionScope scope, WebhookDeadLetterFailure failure, CancellationToken cancellationToken)
    {
        if (_entries.TryGetValue(id, out var entry) && MatchesScope(entry, scope))
        {
            var updated = entry with
            {
                FailureReason = failure.FailureReason,
                StatusCode = failure.StatusCode,
                ErrorDetails = failure.ErrorDetails,
                Attempts = entry.Attempts + 1,
                LastAttemptAtUtc = DateTimeOffset.UtcNow,
                NextAttemptAtUtc = failure.NextAttemptAtUtc
            };
            _entries[id] = updated;
        }

        return Task.CompletedTask;
    }

    public bool Contains(long id) => _entries.ContainsKey(id);

    public void Clear()
    {
        _entries.Clear();
        Interlocked.Exchange(ref _identity, 0);
    }

    private static bool MatchesScope(WebhookDeadLetterEntry entry, PartitionScope scope)
    {
        return string.Equals(entry.TenantId, scope.TenantId, StringComparison.OrdinalIgnoreCase)
            && string.Equals(entry.EnvironmentTag, scope.EnvironmentTag, StringComparison.OrdinalIgnoreCase);
    }
}

/// <summary>
/// In-memory webhook activity store backed by the dead-letter entries.
/// </summary>
public sealed class InMemoryWebhookActivityStore : IWebhookActivityStore
{
    private readonly IWebhookDeadLetterStore _deadLetterStore;

    public InMemoryWebhookActivityStore(IWebhookDeadLetterStore deadLetterStore)
    {
        _deadLetterStore = deadLetterStore ?? throw new ArgumentNullException(nameof(deadLetterStore));
    }

    public async Task<IReadOnlyCollection<WebhookActivityEntry>> ListAsync(
        PartitionScope scope,
        WebhookActivityQuery query,
        CancellationToken cancellationToken)
    {
        if (query is null) throw new ArgumentNullException(nameof(query));

        var normalized = query.Normalize();
        var hookKeys = normalized.HookKeys is null || normalized.HookKeys.Count == 0
            ? null
            : new HashSet<string>(normalized.HookKeys, StringComparer.OrdinalIgnoreCase);
        var jobKeys = normalized.JobKeys is null || normalized.JobKeys.Count == 0
            ? null
            : new HashSet<string>(normalized.JobKeys, StringComparer.OrdinalIgnoreCase);

        var entries = await _deadLetterStore.ListAsync(scope, cancellationToken).ConfigureAwait(false);

        var filtered = entries
            .Where(entry => hookKeys is null || hookKeys.Contains(entry.HookKey))
            .Where(entry => jobKeys is null || jobKeys.Contains(entry.JobKey))
            .Where(entry => !normalized.FromUtc.HasValue || entry.CreatedAtUtc >= normalized.FromUtc.Value)
            .Where(entry => !normalized.ToUtc.HasValue || entry.CreatedAtUtc <= normalized.ToUtc.Value)
            .Where(entry => !normalized.UpdatedSinceUtc.HasValue || entry.CreatedAtUtc >= normalized.UpdatedSinceUtc.Value)
            .OrderByDescending(entry => entry.CreatedAtUtc)
            .ThenByDescending(entry => entry.Id)
            .Take(normalized.Limit)
            .Select(MapDeadLetter)
            .ToArray();

        return filtered;
    }

    public async Task<WebhookActivitySummary> SummarizeAsync(
        PartitionScope scope,
        WebhookActivitySummaryQuery query,
        CancellationToken cancellationToken)
    {
        if (query is null) throw new ArgumentNullException(nameof(query));

        var nowUtc = DateTimeOffset.UtcNow;
        var normalized = query.Normalize(nowUtc);
        var hookKeys = normalized.HookKeys is null || normalized.HookKeys.Count == 0
            ? null
            : new HashSet<string>(normalized.HookKeys, StringComparer.OrdinalIgnoreCase);
        var jobKeys = normalized.JobKeys is null || normalized.JobKeys.Count == 0
            ? null
            : new HashSet<string>(normalized.JobKeys, StringComparer.OrdinalIgnoreCase);

        var windowStartUtc = normalized.FromUtc ?? nowUtc.AddMinutes(-WebhookActivitySummaryQuery.DefaultWindowMinutes);
        var windowEndUtc = normalized.ToUtc ?? nowUtc;
        var bucketMinutes = normalized.BucketMinutes ?? WebhookActivitySummaryQuery.DefaultBucketMinutes;
        if (bucketMinutes <= 0)
        {
            bucketMinutes = WebhookActivitySummaryQuery.DefaultBucketMinutes;
        }

        var entries = await _deadLetterStore.ListAsync(scope, cancellationToken).ConfigureAwait(false);
        var filtered = entries
            .Where(entry => hookKeys is null || hookKeys.Contains(entry.HookKey))
            .Where(entry => jobKeys is null || jobKeys.Contains(entry.JobKey))
            .Where(entry => entry.CreatedAtUtc >= windowStartUtc && entry.CreatedAtUtc <= windowEndUtc)
            .ToArray();

        var buckets = BuildBuckets(filtered, windowStartUtc, windowEndUtc, bucketMinutes);
        return new WebhookActivitySummary(bucketMinutes, windowStartUtc, windowEndUtc, buckets);
    }

    private static IReadOnlyCollection<WebhookActivityBucket> BuildBuckets(
        IReadOnlyCollection<WebhookDeadLetterEntry> entries,
        DateTimeOffset windowStartUtc,
        DateTimeOffset windowEndUtc,
        int bucketMinutes)
    {
        var windowMinutes = Math.Max(1, (windowEndUtc - windowStartUtc).TotalMinutes);
        var bucketCount = Math.Max(1, (int)Math.Ceiling(windowMinutes / bucketMinutes));
        var buckets = new WebhookActivityBucket[bucketCount];

        for (var index = 0; index < bucketCount; index++)
        {
            var start = windowStartUtc.AddMinutes(index * bucketMinutes);
            var end = start.AddMinutes(bucketMinutes);
            if (end > windowEndUtc)
            {
                end = windowEndUtc;
            }

            buckets[index] = new WebhookActivityBucket(
                start,
                end,
                TotalCount: 0,
                ErrorCount: 0,
                WarningCount: 0,
                PendingCount: 0,
                LeasedCount: 0,
                DeadLetterCount: 0,
                P95LatencyMs: null);
        }

        foreach (var entry in entries)
        {
            var offsetMinutes = (entry.CreatedAtUtc - windowStartUtc).TotalMinutes;
            var index = (int)Math.Floor(offsetMinutes / bucketMinutes);
            if (index < 0 || index >= bucketCount)
            {
                continue;
            }

            var bucket = buckets[index];
            buckets[index] = bucket with
            {
                TotalCount = bucket.TotalCount + 1,
                ErrorCount = bucket.ErrorCount + 1,
                DeadLetterCount = bucket.DeadLetterCount + 1
            };
        }

        return buckets;
    }

    private static WebhookActivityEntry MapDeadLetter(WebhookDeadLetterEntry entry)
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
            entry.CreatedAtUtc,
            LatencyMs: null,
            Attempts: entry.Attempts,
            reason,
            ComputePayloadBytes(entry.Payload),
            entry.Id);
    }

    private static int? ComputePayloadBytes(string payload)
    {
        if (string.IsNullOrEmpty(payload))
        {
            return null;
        }

        return Encoding.UTF8.GetByteCount(payload);
    }
}
