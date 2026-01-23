using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Net.Http;
using System.Net.Http.Json;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Croniq.Persistence.Abstractions;

namespace Croniq.Webhooks.Remote;

public sealed class RemoteWebhookClient
{
    private readonly HttpClient _httpClient;
    private readonly JsonSerializerOptions _jsonOptions = new(JsonSerializerDefaults.Web);
    private const string DeliveryPrefix = "delivery:";
    private const string DeadLetterPrefix = "deadletter:";

    public RemoteWebhookClient(HttpClient httpClient)
    {
        _httpClient = httpClient ?? throw new ArgumentNullException(nameof(httpClient));
    }

    public async Task<IReadOnlyCollection<WebhookEndpointDefinition>> ListEndpointsAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        var url = BuildUrl($"tenants/{Escape(scope.TenantId)}/webhooks", scope);
        var payload = await _httpClient
            .GetFromJsonAsync<List<WebhookEndpointResponseDto>>(url, _jsonOptions, cancellationToken)
            .ConfigureAwait(false);

        if (payload is null || payload.Count == 0)
        {
            return Array.Empty<WebhookEndpointDefinition>();
        }

        return payload.Select(entry => MapEndpoint(entry, scope)).ToArray();
    }

    public async Task<WebhookCapabilities> GetCapabilitiesAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        var url = BuildUrl($"tenants/{Escape(scope.TenantId)}/webhooks/capabilities", scope);
        var payload = await _httpClient
            .GetFromJsonAsync<WebhookCapabilitiesResponseDto>(url, _jsonOptions, cancellationToken)
            .ConfigureAwait(false);

        if (payload is null)
        {
            throw new InvalidOperationException("Remote webhook capabilities did not return a response.");
        }

        return new WebhookCapabilities(payload.AllowUnsignedHooks, payload.DefaultRequestsPerMinute);
    }

    public async Task UpsertEndpointAsync(WebhookEndpointUpsert request, CancellationToken cancellationToken)
    {
        var scope = new PartitionScope(request.TenantId, request.EnvironmentTag);
        var url = BuildUrl($"tenants/{Escape(request.TenantId)}/webhooks", scope);

        var payload = new UpsertWebhookEndpointRequestDto(
            request.HookKey,
            request.JobKey,
            request.Enabled,
            request.RequireSignature,
            !request.RequireSignature,
            request.RequestsPerMinute > 0 ? request.RequestsPerMinute : null,
            request.Secret,
            request.Metadata?.ToDictionary(kvp => kvp.Key, kvp => kvp.Value, StringComparer.OrdinalIgnoreCase),
            request.SignatureVersion);

        using var response = await _httpClient
            .PostAsJsonAsync(url, payload, _jsonOptions, cancellationToken)
            .ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
    }

    public async Task DeleteEndpointAsync(string hookKey, PartitionScope scope, bool hardDelete, CancellationToken cancellationToken)
    {
        var query = hardDelete ? new Dictionary<string, string> { ["hardDelete"] = "true" } : null;
        var url = BuildUrl($"tenants/{Escape(scope.TenantId)}/webhooks/{Escape(hookKey)}", scope, query);
        using var response = await _httpClient.DeleteAsync(url, cancellationToken).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
    }

    public async Task<WebhookSecretRotationResult> RotateSecretAsync(WebhookSecretRotate request, CancellationToken cancellationToken)
    {
        var scope = new PartitionScope(request.TenantId, request.EnvironmentTag);
        var url = BuildUrl($"tenants/{Escape(request.TenantId)}/webhooks/{Escape(request.HookKey)}/rotate-secret", scope);
        var payload = new RotateWebhookSecretRequestDto(request.ActivateInSeconds, request.GracePeriodSeconds, request.Notes);

        using var response = await _httpClient
            .PostAsJsonAsync(url, payload, _jsonOptions, cancellationToken)
            .ConfigureAwait(false);
        response.EnsureSuccessStatusCode();

        var result = await response.Content
            .ReadFromJsonAsync<RotateWebhookSecretResponseDto>(_jsonOptions, cancellationToken)
            .ConfigureAwait(false);

        if (result is null)
        {
            throw new InvalidOperationException("Remote webhook rotation did not return a response.");
        }

        return new WebhookSecretRotationResult(
            result.HookKey,
            result.Secret,
            result.SecretHash,
            result.ActivatedAtUtc,
            result.ExpiresAtUtc);
    }

    public async Task<IReadOnlyCollection<WebhookIpRuleDefinition>> ListIpRulesAsync(string hookKey, PartitionScope scope, CancellationToken cancellationToken)
    {
        var url = BuildUrl($"tenants/{Escape(scope.TenantId)}/webhooks/{Escape(hookKey)}/ip-rules", scope);
        var payload = await _httpClient
            .GetFromJsonAsync<List<WebhookIpRuleResponseDto>>(url, _jsonOptions, cancellationToken)
            .ConfigureAwait(false);

        if (payload is null || payload.Count == 0)
        {
            return Array.Empty<WebhookIpRuleDefinition>();
        }

        return payload.Select(rule => MapIpRule(rule, hookKey, scope)).ToArray();
    }

    public async Task<WebhookIpRuleDefinition> AddIpRuleAsync(WebhookIpRuleCreate request, CancellationToken cancellationToken)
    {
        var scope = new PartitionScope(request.TenantId, request.EnvironmentTag);
        var url = BuildUrl($"tenants/{Escape(request.TenantId)}/webhooks/{Escape(request.HookKey)}/ip-rules", scope);
        var payload = new CreateWebhookIpRuleRequestDto(request.Cidr, request.Description);

        using var response = await _httpClient
            .PostAsJsonAsync(url, payload, _jsonOptions, cancellationToken)
            .ConfigureAwait(false);
        response.EnsureSuccessStatusCode();

        var result = await response.Content
            .ReadFromJsonAsync<WebhookIpRuleResponseDto>(_jsonOptions, cancellationToken)
            .ConfigureAwait(false);

        if (result is null)
        {
            throw new InvalidOperationException("Remote webhook IP rule creation did not return a response.");
        }

        return MapIpRule(result, request.HookKey, scope);
    }

    public async Task DeleteIpRuleAsync(string hookKey, long ruleId, PartitionScope scope, CancellationToken cancellationToken)
    {
        var url = BuildUrl($"tenants/{Escape(scope.TenantId)}/webhooks/{Escape(hookKey)}/ip-rules/{ruleId}", scope);
        using var response = await _httpClient.DeleteAsync(url, cancellationToken).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
    }

    public async Task<IReadOnlyCollection<WebhookActivityEntry>> ListActivityAsync(
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

        var queryParams = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
        {
            ["limit"] = normalized.Limit.ToString(CultureInfo.InvariantCulture)
        };

        if (normalized.FromUtc.HasValue)
        {
            queryParams["fromUtc"] = normalized.FromUtc.Value.ToString("O", CultureInfo.InvariantCulture);
        }

        if (normalized.ToUtc.HasValue)
        {
            queryParams["toUtc"] = normalized.ToUtc.Value.ToString("O", CultureInfo.InvariantCulture);
        }

        var hookKeys = NormalizeKeys(normalized.HookKeys);
        if (hookKeys is { Count: > 0 })
        {
            queryParams["hookKeys"] = JoinKeys(hookKeys);
        }

        var jobKeys = NormalizeKeys(normalized.JobKeys);
        if (jobKeys is { Count: > 0 })
        {
            queryParams["jobKeys"] = JoinKeys(jobKeys);
        }

        var url = BuildUrl($"tenants/{Escape(scope.TenantId)}/webhooks/activity", scope, queryParams);
        var payload = await _httpClient
            .GetFromJsonAsync<List<WebhookActivityTimelineResponseDto>>(url, _jsonOptions, cancellationToken)
            .ConfigureAwait(false);

        if (payload is null || payload.Count == 0)
        {
            return Array.Empty<WebhookActivityEntry>();
        }

        return payload.Select(entry => MapActivityTimelineEntry(entry, scope)).ToArray();
    }

    public async Task<WebhookActivitySummary> SummarizeActivityAsync(
        PartitionScope scope,
        WebhookActivitySummaryQuery query,
        CancellationToken cancellationToken)
    {
        if (query is null) throw new ArgumentNullException(nameof(query));

        var normalized = query.Normalize(DateTimeOffset.UtcNow);
        var bucketMinutes = normalized.BucketMinutes ?? WebhookActivitySummaryQuery.DefaultBucketMinutes;
        if (bucketMinutes <= 0)
        {
            bucketMinutes = WebhookActivitySummaryQuery.DefaultBucketMinutes;
        }

        var queryParams = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
        {
            ["bucketMinutes"] = bucketMinutes.ToString(CultureInfo.InvariantCulture)
        };

        if (normalized.FromUtc.HasValue)
        {
            queryParams["fromUtc"] = normalized.FromUtc.Value.ToString("O", CultureInfo.InvariantCulture);
        }

        if (normalized.ToUtc.HasValue)
        {
            queryParams["toUtc"] = normalized.ToUtc.Value.ToString("O", CultureInfo.InvariantCulture);
        }

        var hookKeys = NormalizeKeys(normalized.HookKeys);
        if (hookKeys is { Count: > 0 })
        {
            queryParams["hookKeys"] = JoinKeys(hookKeys);
        }

        var jobKeys = NormalizeKeys(normalized.JobKeys);
        if (jobKeys is { Count: > 0 })
        {
            queryParams["jobKeys"] = JoinKeys(jobKeys);
        }

        var url = BuildUrl($"tenants/{Escape(scope.TenantId)}/webhooks/activity/summary", scope, queryParams);
        var payload = await _httpClient
            .GetFromJsonAsync<WebhookActivitySummaryResponseDto>(url, _jsonOptions, cancellationToken)
            .ConfigureAwait(false);

        if (payload is null)
        {
            throw new InvalidOperationException("Remote webhook activity summary did not return a response.");
        }

        return MapActivitySummary(payload);
    }

    public async Task<IReadOnlyCollection<WebhookDeadLetterEntry>> ListDeadLettersAsync(PartitionScope scope, CancellationToken cancellationToken)
    {
        var url = BuildUrl($"tenants/{Escape(scope.TenantId)}/webhooks/deadletters", scope);
        var payload = await _httpClient
            .GetFromJsonAsync<List<WebhookDeadLetterResponseDto>>(url, _jsonOptions, cancellationToken)
            .ConfigureAwait(false);

        if (payload is null || payload.Count == 0)
        {
            return Array.Empty<WebhookDeadLetterEntry>();
        }

        return payload.Select(MapDeadLetter).ToArray();
    }

    public async Task ResolveDeadLetterAsync(long deadLetterId, PartitionScope scope, CancellationToken cancellationToken)
    {
        var url = BuildUrl($"tenants/{Escape(scope.TenantId)}/webhooks/deadletters/{deadLetterId}:resolve", scope);
        using var response = await _httpClient.PostAsync(url, content: null, cancellationToken).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
    }

    public async Task RecordDeadLetterFailureAsync(long deadLetterId, PartitionScope scope, WebhookDeadLetterFailure failure, CancellationToken cancellationToken)
    {
        var url = BuildUrl($"tenants/{Escape(scope.TenantId)}/webhooks/deadletters/{deadLetterId}:fail", scope);
        var payload = new WebhookDeadLetterFailureRequestDto(
            failure.FailureReason,
            failure.StatusCode,
            failure.ErrorDetails,
            failure.NextAttemptAtUtc);

        using var response = await _httpClient
            .PostAsJsonAsync(url, payload, _jsonOptions, cancellationToken)
            .ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
    }

    private static string BuildUrl(string path, PartitionScope scope, IReadOnlyDictionary<string, string>? extraQuery = null)
    {
        var builder = new System.Text.StringBuilder(path);
        var hasQuery = false;

        void Append(string key, string? value)
        {
            if (string.IsNullOrWhiteSpace(key) || string.IsNullOrWhiteSpace(value))
            {
                return;
            }

            builder.Append(hasQuery ? '&' : '?');
            builder.Append(Escape(key));
            builder.Append('=');
            builder.Append(Escape(value));
            hasQuery = true;
        }

        Append("environment", scope.EnvironmentTag);
        if (extraQuery is not null)
        {
            foreach (var entry in extraQuery)
            {
                Append(entry.Key, entry.Value);
            }
        }

        return builder.ToString();
    }

    private static string Escape(string value) => Uri.EscapeDataString(value);

    private static WebhookEndpointDefinition MapEndpoint(WebhookEndpointResponseDto entry, PartitionScope scope)
    {
        var ipRules = entry.IpRules is null || entry.IpRules.Count == 0
            ? Array.Empty<WebhookIpRuleDefinition>()
            : entry.IpRules.Select(rule => MapIpRule(rule, entry.HookKey, scope)).ToArray();

        return new WebhookEndpointDefinition(
            entry.HookKey,
            entry.JobKey,
            entry.Secret ?? string.Empty,
            entry.Enabled,
            entry.RequireSignature,
            entry.RequestsPerMinute,
            scope.TenantId,
            scope.EnvironmentTag,
            ToReadOnlyDictionary(entry.Metadata),
            ipRules,
            SignatureVersion: 1,
            entry.CreatedAtUtc,
            entry.UpdatedAtUtc);
    }

    private static WebhookIpRuleDefinition MapIpRule(WebhookIpRuleResponseDto rule, string hookKey, PartitionScope scope)
    {
        return new WebhookIpRuleDefinition(
            rule.Id,
            hookKey,
            scope.TenantId,
            scope.EnvironmentTag,
            rule.Cidr,
            rule.Description,
            rule.CreatedBy,
            rule.CreatedAtUtc,
            rule.UpdatedAtUtc);
    }

    private static WebhookDeadLetterEntry MapDeadLetter(WebhookDeadLetterResponseDto entry)
    {
        return new WebhookDeadLetterEntry(
            entry.Id,
            entry.HookKey,
            entry.JobKey,
            entry.TenantId,
            entry.EnvironmentTag,
            entry.Payload,
            ToReadOnlyDictionary(entry.Headers),
            ToReadOnlyDictionary(entry.Metadata),
            entry.FailureReason,
            entry.Attempts,
            entry.StatusCode,
            entry.ErrorDetails,
            entry.CreatedAtUtc,
            entry.LastAttemptAtUtc,
            entry.NextAttemptAtUtc,
            entry.ExpiresAtUtc);
    }

    private static WebhookActivityEntry MapActivityTimelineEntry(WebhookActivityTimelineResponseDto entry, PartitionScope scope)
    {
        var kind = ParseActivityKind(entry.Kind);
        var status = ParseActivityStatus(entry.Status);
        var source = string.IsNullOrWhiteSpace(entry.Source) ? WebhookActivitySources.Ingress : entry.Source;
        var environment = string.IsNullOrWhiteSpace(entry.Environment) ? scope.EnvironmentTag : entry.Environment!;
        var id = ResolveActivityId(entry, kind);
        var deadLetterId = ResolveDeadLetterId(entry, kind);

        return new WebhookActivityEntry(
            id,
            kind,
            status,
            entry.HookKey,
            entry.JobKey,
            scope.TenantId,
            environment,
            source,
            entry.OccurredAtUtc,
            entry.Reason,
            entry.PayloadBytes,
            deadLetterId);
    }

    private static WebhookActivitySummary MapActivitySummary(WebhookActivitySummaryResponseDto payload)
    {
        var bucketMinutes = payload.BucketMinutes > 0
            ? payload.BucketMinutes
            : WebhookActivitySummaryQuery.DefaultBucketMinutes;
        if (bucketMinutes <= 0)
        {
            bucketMinutes = 1;
        }

        var buckets = payload.Buckets is null || payload.Buckets.Count == 0
            ? Array.Empty<WebhookActivityBucket>()
            : payload.Buckets.Select(bucket => new WebhookActivityBucket(
                bucket.BucketStartUtc,
                bucket.BucketEndUtc ?? bucket.BucketStartUtc.AddMinutes(bucketMinutes),
                bucket.TotalCount,
                bucket.ErrorCount,
                bucket.WarningCount,
                bucket.PendingCount,
                bucket.LeasedCount,
                bucket.DeadLetterCount,
                bucket.P95LatencyMs))
            .ToArray();

        return new WebhookActivitySummary(
            bucketMinutes,
            payload.WindowStartUtc,
            payload.WindowEndUtc,
            buckets);
    }

    private static WebhookActivityKind ParseActivityKind(string? value)
    {
        if (string.Equals(value, "deadLetter", StringComparison.OrdinalIgnoreCase)
            || string.Equals(value, "deadletter", StringComparison.OrdinalIgnoreCase))
        {
            return WebhookActivityKind.DeadLetter;
        }

        return WebhookActivityKind.Delivery;
    }

    private static WebhookActivityStatus ParseActivityStatus(string? value)
    {
        if (string.Equals(value, "failed", StringComparison.OrdinalIgnoreCase))
        {
            return WebhookActivityStatus.Failed;
        }

        if (string.Equals(value, "warning", StringComparison.OrdinalIgnoreCase))
        {
            return WebhookActivityStatus.Warning;
        }

        if (string.Equals(value, "pending", StringComparison.OrdinalIgnoreCase))
        {
            return WebhookActivityStatus.Pending;
        }

        if (string.Equals(value, "leased", StringComparison.OrdinalIgnoreCase))
        {
            return WebhookActivityStatus.Leased;
        }

        return WebhookActivityStatus.Success;
    }

    private static string ResolveActivityId(WebhookActivityTimelineResponseDto entry, WebhookActivityKind kind)
    {
        if (kind == WebhookActivityKind.DeadLetter)
        {
            if (entry.DeadLetterId.HasValue)
            {
                return entry.DeadLetterId.Value.ToString(CultureInfo.InvariantCulture);
            }

            if (!string.IsNullOrWhiteSpace(entry.Id))
            {
                var trimmed = StripPrefix(entry.Id, DeadLetterPrefix);
                if (!string.IsNullOrWhiteSpace(trimmed))
                {
                    return trimmed;
                }
            }

            return entry.Id ?? string.Empty;
        }

        if (!string.IsNullOrWhiteSpace(entry.RequestId))
        {
            return entry.RequestId;
        }

        if (!string.IsNullOrWhiteSpace(entry.Id))
        {
            var trimmed = StripPrefix(entry.Id, DeliveryPrefix);
            if (!string.IsNullOrWhiteSpace(trimmed))
            {
                return trimmed;
            }
        }

        return entry.Id ?? string.Empty;
    }

    private static long? ResolveDeadLetterId(WebhookActivityTimelineResponseDto entry, WebhookActivityKind kind)
    {
        if (kind != WebhookActivityKind.DeadLetter)
        {
            return null;
        }

        if (entry.DeadLetterId.HasValue)
        {
            return entry.DeadLetterId.Value;
        }

        if (string.IsNullOrWhiteSpace(entry.Id))
        {
            return null;
        }

        var trimmed = StripPrefix(entry.Id, DeadLetterPrefix);
        return long.TryParse(trimmed, NumberStyles.Integer, CultureInfo.InvariantCulture, out var id) ? id : null;
    }

    private static string StripPrefix(string value, string prefix)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            return value;
        }

        return value.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)
            ? value[prefix.Length..]
            : value;
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

    private static string JoinKeys(IReadOnlyCollection<string> values)
    {
        return string.Join(",", values);
    }

    private static IReadOnlyDictionary<string, string>? ToReadOnlyDictionary(IDictionary<string, string>? values)
    {
        if (values is null || values.Count == 0)
        {
            return null;
        }

        return new Dictionary<string, string>(values);
    }

    private sealed record UpsertWebhookEndpointRequestDto(
        string HookKey,
        string JobKey,
        bool Enabled,
        bool RequireSignature,
        bool AllowUnsigned,
        int? RequestsPerMinute,
        string? Secret,
        IDictionary<string, string>? Metadata,
        int SignatureVersion);

    private sealed record WebhookEndpointResponseDto(
        string HookKey,
        string JobKey,
        bool Enabled,
        bool RequireSignature,
        int RequestsPerMinute,
        IDictionary<string, string>? Metadata,
        IReadOnlyCollection<WebhookIpRuleResponseDto> IpRules,
        DateTimeOffset CreatedAtUtc,
        DateTimeOffset UpdatedAtUtc,
        string? Secret = null);

    private sealed record RotateWebhookSecretRequestDto(
        int? ActivateInSeconds,
        int? GracePeriodSeconds,
        string? Notes);

    private sealed record RotateWebhookSecretResponseDto(
        string HookKey,
        DateTime ActivatedAtUtc,
        DateTime? ExpiresAtUtc,
        string Secret,
        string SecretHash);

    private sealed record CreateWebhookIpRuleRequestDto(
        string Cidr,
        string? Description = null);

    private sealed record WebhookIpRuleResponseDto(
        long Id,
        string Cidr,
        string? Description,
        string? CreatedBy,
        DateTimeOffset CreatedAtUtc,
        DateTimeOffset UpdatedAtUtc);

    private sealed record WebhookDeadLetterResponseDto(
        long Id,
        string HookKey,
        string JobKey,
        string TenantId,
        string EnvironmentTag,
        string Payload,
        IDictionary<string, string>? Headers,
        IDictionary<string, string>? Metadata,
        string FailureReason,
        int Attempts,
        int? StatusCode,
        string? ErrorDetails,
        DateTimeOffset CreatedAtUtc,
        DateTimeOffset? LastAttemptAtUtc,
        DateTimeOffset? NextAttemptAtUtc,
        DateTimeOffset? ExpiresAtUtc);

    private sealed record WebhookDeadLetterFailureRequestDto(
        string FailureReason,
        int? StatusCode,
        string? ErrorDetails,
        DateTimeOffset? NextAttemptAtUtc);

    private sealed record WebhookActivityTimelineResponseDto(
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

    private sealed record WebhookActivitySummaryResponseDto(
        int BucketMinutes,
        DateTimeOffset WindowStartUtc,
        DateTimeOffset WindowEndUtc,
        IReadOnlyCollection<WebhookActivityBucketResponseDto> Buckets);

    private sealed record WebhookActivityBucketResponseDto(
        DateTimeOffset BucketStartUtc,
        DateTimeOffset? BucketEndUtc,
        int TotalCount,
        int ErrorCount,
        int WarningCount,
        int PendingCount,
        int LeasedCount,
        int DeadLetterCount,
        int? P95LatencyMs);

    private sealed record WebhookCapabilitiesResponseDto(
        bool AllowUnsignedHooks,
        int DefaultRequestsPerMinute);
}
