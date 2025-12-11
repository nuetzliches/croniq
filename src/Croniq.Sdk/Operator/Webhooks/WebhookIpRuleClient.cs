using System;
using System.Globalization;
using System.Net.Http;
using System.Net.Http.Json;
using System.Text.Json;
using Microsoft.Extensions.Logging;

namespace Croniq.Sdk.Operator.Webhooks;

public sealed class WebhookIpRuleClient
{
    private static readonly JsonSerializerOptions SerializerOptions = new(JsonSerializerDefaults.Web);
    private const string CorrelationHeaderName = "X-Croniq-CorrelationId";

    private readonly HttpClient _httpClient;
    private readonly ILogger<WebhookIpRuleClient>? _logger;

    public WebhookIpRuleClient(HttpClient httpClient, ILogger<WebhookIpRuleClient>? logger = null)
    {
        _httpClient = httpClient ?? throw new ArgumentNullException(nameof(httpClient));
        _logger = logger;
    }

    public async Task<IReadOnlyList<WebhookIpRule>> ListAsync(
        string tenantId,
        string hookKey,
        string environment,
        string? correlationId = null,
        CancellationToken cancellationToken = default)
    {
        var result = await SendAsync<List<WebhookIpRule>>(
                HttpMethod.Get,
                BuildRulesUrl(tenantId, hookKey, environment),
                null,
                correlationId,
                cancellationToken)
            .ConfigureAwait(false);
        return result ?? new List<WebhookIpRule>();
    }

    public Task<WebhookIpRule?> CreateAsync(
        string tenantId,
        string hookKey,
        string environment,
        WebhookIpRuleCreateRequest request,
        string? correlationId = null,
        CancellationToken cancellationToken = default)
    {
        _ = request ?? throw new ArgumentNullException(nameof(request));

        return SendAsync<WebhookIpRule>(
            HttpMethod.Post,
            BuildRulesUrl(tenantId, hookKey, environment),
            request,
            correlationId,
            cancellationToken);
    }

    public async Task DeleteAsync(
        string tenantId,
        string hookKey,
        long ruleId,
        string environment,
        string? correlationId = null,
        CancellationToken cancellationToken = default)
    {
        var url = BuildRuleDeleteUrl(tenantId, hookKey, ruleId, environment);
        using var request = new HttpRequestMessage(HttpMethod.Delete, url);
        ApplyCorrelationHeader(request, correlationId);
        using var response = await _httpClient.SendAsync(request, cancellationToken).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
        {
            throw await CroniqApiException.FromResponseAsync(response, cancellationToken).ConfigureAwait(false);
        }
    }

    public async Task<WebhookIpRuleSyncResult> SyncAsync(
        string tenantId,
        string hookKey,
        string environment,
        IEnumerable<WebhookIpRuleDesired> desiredRules,
        string? correlationId = null,
        CancellationToken cancellationToken = default)
    {
        _ = desiredRules ?? throw new ArgumentNullException(nameof(desiredRules));

        var syncCorrelationId = string.IsNullOrWhiteSpace(correlationId)
            ? Guid.NewGuid().ToString("N")
            : correlationId;

        var desiredMap = BuildDesiredMap(desiredRules);
        var existing = await ListAsync(tenantId, hookKey, environment, syncCorrelationId, cancellationToken).ConfigureAwait(false);
        var existingMap = existing.ToDictionary(
            static rule => NormalizeCidr(rule.Cidr),
            static rule => rule,
            StringComparer.OrdinalIgnoreCase);

        var created = new List<WebhookIpRule>();

        foreach (var desired in desiredMap.Values)
        {
            var normalizedCidr = NormalizeCidr(desired.Cidr);
            if (normalizedCidr.Length == 0)
            {
                continue;
            }

            if (existingMap.ContainsKey(normalizedCidr))
            {
                continue;
            }

            _logger?.LogInformation(
                "Creating webhook IP rule for hook {HookKey} with CIDR {Cidr}",
                hookKey,
                normalizedCidr);

            var createdRule = await CreateAsync(
                    tenantId,
                    hookKey,
                    environment,
                    new WebhookIpRuleCreateRequest(normalizedCidr, desired.Description),
                    syncCorrelationId,
                    cancellationToken)
                .ConfigureAwait(false);
            if (createdRule is not null)
            {
                created.Add(createdRule);
                existingMap[NormalizeCidr(createdRule.Cidr)] = createdRule;
            }
        }

        var deletedIds = new List<long>();
        foreach (var rule in existing)
        {
            var normalized = NormalizeCidr(rule.Cidr);
            if (normalized.Length == 0 || desiredMap.ContainsKey(normalized))
            {
                continue;
            }

            _logger?.LogInformation(
                "Deleting webhook IP rule {RuleId} ({Cidr}) for hook {HookKey}",
                rule.Id,
                rule.Cidr,
                hookKey);

            await DeleteAsync(
                    tenantId,
                    hookKey,
                    rule.Id,
                    environment,
                    syncCorrelationId,
                    cancellationToken)
                .ConfigureAwait(false);
            deletedIds.Add(rule.Id);
            existingMap.Remove(normalized);
        }

        var finalState = await ListAsync(tenantId, hookKey, environment, syncCorrelationId, cancellationToken).ConfigureAwait(false);
        return new WebhookIpRuleSyncResult(created, deletedIds, finalState);
    }

    private async Task<T?> SendAsync<T>(HttpMethod method, string url, object? payload, string? correlationId, CancellationToken cancellationToken)
    {
        using var request = new HttpRequestMessage(method, url);
        if (payload is not null)
        {
            request.Content = JsonContent.Create(payload, options: SerializerOptions);
        }

        ApplyCorrelationHeader(request, correlationId);

        using var response = await _httpClient.SendAsync(request, cancellationToken).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode)
        {
            throw await CroniqApiException.FromResponseAsync(response, cancellationToken).ConfigureAwait(false);
        }

        if (response.Content is null)
        {
            return default;
        }

        try
        {
            return await response.Content.ReadFromJsonAsync<T>(SerializerOptions, cancellationToken).ConfigureAwait(false);
        }
        catch (NotSupportedException ex)
        {
            throw new InvalidOperationException("Failed to deserialize Croniq API response payload.", ex);
        }
    }

    private static IReadOnlyDictionary<string, WebhookIpRuleDesired> BuildDesiredMap(IEnumerable<WebhookIpRuleDesired> desired)
    {
        var map = new Dictionary<string, WebhookIpRuleDesired>(StringComparer.OrdinalIgnoreCase);
        foreach (var rule in desired)
        {
            if (string.IsNullOrWhiteSpace(rule?.Cidr))
            {
                continue;
            }

            var normalized = NormalizeCidr(rule.Cidr);
            if (normalized.Length == 0)
            {
                continue;
            }

            map[normalized] = rule with { Cidr = normalized };
        }

        return map;
    }

    private static string BuildRulesUrl(string tenantId, string hookKey, string environment)
    {
        return $"/tenants/{EscapeSegment(tenantId)}/webhooks/{EscapeSegment(hookKey)}/ip-rules?environment={Uri.EscapeDataString(environment ?? string.Empty)}";
    }

    private static string BuildRuleDeleteUrl(string tenantId, string hookKey, long ruleId, string environment)
    {
        return $"/tenants/{EscapeSegment(tenantId)}/webhooks/{EscapeSegment(hookKey)}/ip-rules/{ruleId.ToString(CultureInfo.InvariantCulture)}?environment={Uri.EscapeDataString(environment ?? string.Empty)}";
    }

    private static string EscapeSegment(string value)
    {
        if (string.IsNullOrWhiteSpace(value))
        {
            throw new ArgumentException("Value cannot be null or whitespace.", nameof(value));
        }

        return Uri.EscapeDataString(value);
    }

    private static string NormalizeCidr(string cidr)
    {
        if (string.IsNullOrWhiteSpace(cidr))
        {
            return string.Empty;
        }

        return cidr.Trim();
    }

    private static void ApplyCorrelationHeader(HttpRequestMessage request, string? correlationId)
    {
        if (string.IsNullOrWhiteSpace(correlationId))
        {
            return;
        }

        request.Headers.Remove(CorrelationHeaderName);
        request.Headers.TryAddWithoutValidation(CorrelationHeaderName, correlationId);
    }
}
