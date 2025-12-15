using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Net.Http.Json;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;
using Shouldly;
using Xunit;

namespace Croniq.Api.Smoke;

public sealed class SmokeTests
{
    private static readonly SmokeTestConfiguration Config = SmokeTestConfiguration.Load();
    private static readonly JsonSerializerOptions JsonOptions = new(JsonSerializerDefaults.Web);
    private static readonly string SampleLoggingJobKey = $"{Config.TenantId}:{Config.EnvironmentTag}:samples:smoke";
    private static bool SmokeTestsDisabled => !SmokeTestConfiguration.IsEnabled;
    private static readonly bool IsCiAgent = string.Equals(Environment.GetEnvironmentVariable("TF_BUILD"), "true", StringComparison.OrdinalIgnoreCase)
        || string.Equals(Environment.GetEnvironmentVariable("CI"), "true", StringComparison.OrdinalIgnoreCase);
    private static readonly string SmokeLogPrefix = "[Croniq.Api.Smoke]";

    [Fact]
    public async Task Health_endpoint_reports_ok()
    {
        if (SmokeTestsDisabled)
        {
            return;
        }

        if (IsCiAgent)
        {
            SkipDueToAgentLimitations();
            return;
        }

        using var client = CreateClient();
        using var response = await SendAsync(() => client.GetAsync("health"));

        response.StatusCode.ShouldBe(HttpStatusCode.OK);
        var body = await response.Content.ReadFromJsonAsync<HealthResponse>();
        body.ShouldNotBeNull();
        body!.Status.ShouldBe("ok");
    }

    [Fact]
    public async Task Schedule_endpoint_accepts_new_jobs()
    {
        if (SmokeTestsDisabled)
        {
            return;
        }

        if (IsCiAgent)
        {
            SkipDueToAgentLimitations();
            return;
        }

        using var client = CreateClient();
        var jobKey = BuildJobKey("schedules");
        var payload = CreateSchedulePayload(jobKey);

        using var response = await SendAsync(() => client.PostAsJsonAsync(GetSchedulesUrl(), payload));

        response.StatusCode.ShouldBe(HttpStatusCode.Created);
        var body = await response.Content.ReadFromJsonAsync<ScheduleResponse>();
        body.ShouldNotBeNull();
        body!.TriggerId.ShouldNotBeNullOrWhiteSpace();
        body.JobKey.ShouldBe(jobKey);
        body.ScheduleExpression.ShouldBe(payload.CronExpression);
    }

    [Fact]
    public async Task Webhook_ip_rule_crud_roundtrip_succeeds()
    {
        if (SmokeTestsDisabled)
        {
            return;
        }

        if (IsCiAgent)
        {
            SkipDueToAgentLimitations();
            return;
        }

        using var client = CreateClient();
        var jobKey = BuildJobKey("webhooks");

        await UpsertScheduleAsync(client, jobKey);

        var hookKey = $"hook-smoke-{Guid.NewGuid():N}";
        var secret = $"whsec_{Guid.NewGuid():N}";
        var webhook = await UpsertWebhookAsync(client, hookKey, jobKey, secret);
        webhook.IpRules.ShouldBeEmpty();

        try
        {
            var cidr = "203.0.113.0/28";
            var rule = await CreateIpRuleAsync(client, hookKey, cidr, "smoke-allow");

            using var listResponse = await SendAsync(() => client.GetAsync(GetWebhookIpRulesUrl(hookKey)));
            listResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
            var rules = await listResponse.Content.ReadFromJsonAsync<List<WebhookIpRuleResponse>>() ?? new List<WebhookIpRuleResponse>();
            var singleRule = rules.ShouldHaveSingleItem();
            singleRule.Id.ShouldBe(rule.Id);
            singleRule.Cidr.ShouldBe(cidr);

            await DeleteIpRuleAsync(client, hookKey, rule.Id);

            using var finalListResponse = await SendAsync(() => client.GetAsync(GetWebhookIpRulesUrl(hookKey)));
            finalListResponse.StatusCode.ShouldBe(HttpStatusCode.OK);
            var finalRules = await finalListResponse.Content.ReadFromJsonAsync<List<WebhookIpRuleResponse>>() ?? new List<WebhookIpRuleResponse>();
            finalRules.ShouldBeEmpty();
        }
        finally
        {
            await CleanupWebhookAsync(client, hookKey);
        }
    }

    private static void SkipDueToAgentLimitations()
    {
        Console.WriteLine($"{SmokeLogPrefix} Docker/Testcontainers smoke tests skipped on CI agent. Run locally with CRONIQ_RUN_SMOKE_TESTS=1 when the Croniq dev stack is running.");
    }

    [Fact]
    public async Task Webhook_ingress_respects_ip_rules()
    {
        if (SmokeTestsDisabled)
        {
            return;
        }

        using var client = CreateClient();
        var jobKey = SampleLoggingJobKey;

        await UpsertScheduleAsync(client, jobKey);

        var hookKey = $"hook-smoke-{Guid.NewGuid():N}";
        var secret = $"whsec_{Guid.NewGuid():N}";
        await UpsertWebhookAsync(client, hookKey, jobKey, secret);

        try
        {
            var denyRule = await CreateIpRuleAsync(client, hookKey, "203.0.113.0/28", "deny-vnet");

            using (var webhookClient = CreateWebhookClient())
            {
                var payload = new { requestId = Guid.NewGuid().ToString("N"), scope = "blocked" };
                using var blockedResponse = await SendWebhookAsync(webhookClient, hookKey, secret, payload);
                blockedResponse.StatusCode.ShouldBe(HttpStatusCode.Forbidden);
            }

            await DeleteIpRuleAsync(client, hookKey, denyRule.Id);
            await CreateIpRuleAsync(client, hookKey, "0.0.0.0/0", "allow-any-v4");
            await CreateIpRuleAsync(client, hookKey, "::/0", "allow-any-v6");

            using (var webhookClient = CreateWebhookClient())
            {
                var payload = new { requestId = Guid.NewGuid().ToString("N"), scope = "allowed" };
                using var allowedResponse = await SendWebhookAsync(webhookClient, hookKey, secret, payload);
                allowedResponse.StatusCode.ShouldBe(HttpStatusCode.Accepted);
                var body = await allowedResponse.Content.ReadFromJsonAsync<WebhookTriggerResponse>();
                body.ShouldNotBeNull();
                body!.Status.ShouldBe("triggered");
                body.Hook.ShouldBe(hookKey);
                body.Job.ShouldBe(jobKey);
            }
        }
        finally
        {
            await CleanupWebhookAsync(client, hookKey);
        }
    }

    private static async Task UpsertScheduleAsync(HttpClient client, string jobKey)
    {
        using var response = await SendAsync(() => client.PostAsJsonAsync(GetSchedulesUrl(), CreateSchedulePayload(jobKey)));
        response.EnsureSuccessStatusCode();
    }

    private static string BuildJobKey(string scenario) =>
        $"{Config.TenantId}:{Config.EnvironmentTag}:samples:{scenario}-{Guid.NewGuid():N}";

    private static string GetWebhookCollectionUrl(bool allowUnsigned = false) =>
        $"tenants/{Config.TenantId}/webhooks?environment={Config.EnvironmentTag}&allowUnsigned={allowUnsigned.ToString().ToLowerInvariant()}";

    private static string GetSchedulesUrl() => $"tenants/{Config.TenantId}/schedules";

    private static string GetWebhookResourceUrl(string hookKey) =>
        $"tenants/{Config.TenantId}/webhooks/{hookKey}?environment={Config.EnvironmentTag}";

    private static string GetWebhookIpRulesUrl(string hookKey) =>
        $"tenants/{Config.TenantId}/webhooks/{hookKey}/ip-rules?environment={Config.EnvironmentTag}";

    private static string GetWebhookIpRuleDeleteUrl(string hookKey, long ruleId) =>
        $"tenants/{Config.TenantId}/webhooks/{hookKey}/ip-rules/{ruleId}?environment={Config.EnvironmentTag}";

    private static UpsertSchedulePayload CreateSchedulePayload(string jobKey) =>
        new(
            JobKey: jobKey,
            CronExpression: "0/5 * * * * ?",
            Description: "smoke-test",
            Metadata: new Dictionary<string, string>
            {
                ["source"] = "Croniq.Api.Smoke"
            });

    private static HttpClient CreateClient()
    {
        var client = new HttpClient
        {
            BaseAddress = new Uri(Config.BaseUrl, UriKind.Absolute)
        };
        client.DefaultRequestHeaders.Add("X-Croniq-Key", Config.ApiKey);
        return client;
    }

    private static HttpClient CreateWebhookClient()
    {
        return new HttpClient
        {
            BaseAddress = new Uri(Config.WebhookBaseUrl, UriKind.Absolute)
        };
    }

    private static async Task<HttpResponseMessage> SendAsync(Func<Task<HttpResponseMessage>> action)
    {
        try
        {
            return await action().ConfigureAwait(false);
        }
        catch (HttpRequestException ex)
        {
            throw new InvalidOperationException(
                "Croniq.Api smoke endpoint is unreachable. Ensure docker compose up --build is running (see docs-deep-dive/testing.md).",
                ex);
        }
    }

    private sealed record HealthResponse(string Status);

    private sealed record ScheduleResponse(string TriggerId, string JobKey, string ScheduleExpression);

    private sealed record UpsertSchedulePayload(
        string JobKey,
        string CronExpression,
        string? TriggerId = null,
        DateTimeOffset? StartAtUtc = null,
        DateTimeOffset? EndAtUtc = null,
        bool Enabled = true,
        string? Description = null,
        Dictionary<string, string>? Metadata = null);

    private static async Task<WebhookEndpointResponse> UpsertWebhookAsync(HttpClient client, string hookKey, string jobKey, string secret)
    {
        var upsert = new UpsertWebhookEndpointRequest(
            HookKey: hookKey,
            JobKey: jobKey,
            Enabled: true,
            RequireSignature: true,
            RequestsPerMinute: 30,
            Secret: secret,
            Metadata: new Dictionary<string, string>
            {
                ["source"] = "Croniq.Api.Smoke"
            },
            SignatureVersion: 1);

        using var response = await SendAsync(() => client.PostAsJsonAsync(GetWebhookCollectionUrl(), upsert));
        var content = await response.Content.ReadAsStringAsync();
        response.StatusCode.ShouldBe(HttpStatusCode.OK, $"response: {content}");
        var body = JsonSerializer.Deserialize<WebhookEndpointResponse>(content, JsonOptions);
        body.ShouldNotBeNull($"response: {content}");
        return body!;
    }

    private static async Task<WebhookIpRuleResponse> CreateIpRuleAsync(HttpClient client, string hookKey, string cidr, string description)
    {
        var ruleRequest = new CreateWebhookIpRuleRequest(cidr, description);
        using var response = await SendAsync(() => client.PostAsJsonAsync(GetWebhookIpRulesUrl(hookKey), ruleRequest));
        var content = await response.Content.ReadAsStringAsync();
        response.StatusCode.ShouldBe(HttpStatusCode.OK, $"response: {content}");
        var rule = JsonSerializer.Deserialize<WebhookIpRuleResponse>(content, JsonOptions);
        rule.ShouldNotBeNull($"response: {content}");
        return rule!;
    }

    private static async Task DeleteIpRuleAsync(HttpClient client, string hookKey, long ruleId)
    {
        using var response = await SendAsync(() => client.DeleteAsync(GetWebhookIpRuleDeleteUrl(hookKey, ruleId)));
        response.StatusCode.ShouldBe(HttpStatusCode.NoContent);
    }

    private static async Task CleanupWebhookAsync(HttpClient client, string hookKey)
    {
        using var response = await SendAsync(() => client.DeleteAsync(GetWebhookResourceUrl(hookKey)));
        if (response.StatusCode != HttpStatusCode.NoContent && response.StatusCode != HttpStatusCode.NotFound)
        {
            response.EnsureSuccessStatusCode();
        }
    }

    private sealed record UpsertWebhookEndpointRequest(
        string HookKey,
        string JobKey,
        bool Enabled,
        bool RequireSignature,
        int? RequestsPerMinute,
        string? Secret,
        Dictionary<string, string>? Metadata,
        int SignatureVersion);

    private sealed record WebhookEndpointResponse(
        string HookKey,
        string JobKey,
        bool Enabled,
        bool RequireSignature,
        int RequestsPerMinute,
        Dictionary<string, string>? Metadata,
        IReadOnlyCollection<WebhookIpRuleResponse> IpRules,
        DateTimeOffset CreatedAtUtc,
        DateTimeOffset UpdatedAtUtc,
        string? Secret);

    private sealed record CreateWebhookIpRuleRequest(string Cidr, string? Description);

    private sealed record WebhookIpRuleResponse(
        long Id,
        string Cidr,
        string? Description,
        string? CreatedBy,
        DateTimeOffset CreatedAtUtc,
        DateTimeOffset UpdatedAtUtc);

    private sealed record WebhookTriggerResponse(string Status, string Hook, string Job);

    private static Task<HttpResponseMessage> SendWebhookAsync(HttpClient client, string hookKey, string secret, object payload)
    {
        return SendAsync(async () =>
        {
            var payloadJson = JsonSerializer.Serialize(payload);
            using var request = new HttpRequestMessage(HttpMethod.Post, $"webhooks/{hookKey}")
            {
                Content = new StringContent(payloadJson, Encoding.UTF8, "application/json")
            };
            request.Headers.Add("X-Croniq-Signature", ComputeSignature(secret, payloadJson));
            return await client.SendAsync(request).ConfigureAwait(false);
        });
    }

    private static string ComputeSignature(string secret, string payload)
    {
        var keyBytes = Encoding.UTF8.GetBytes(secret);
        var payloadBytes = Encoding.UTF8.GetBytes(payload ?? string.Empty);
        var hash = HMACSHA256.HashData(keyBytes, payloadBytes);
        return $"sha256={Convert.ToHexString(hash).ToLowerInvariant()}";
    }
}
