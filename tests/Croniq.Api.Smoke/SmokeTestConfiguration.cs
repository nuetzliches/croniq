using System;

namespace Croniq.Api.Smoke;

internal sealed record SmokeTestConfiguration(string BaseUrl, string ApiKey, string TenantId, string EnvironmentTag, string WebhookBaseUrl)
{
    private const string EnableFlag = "CRONIQ_RUN_SMOKE_TESTS";

    public static bool IsEnabled =>
        string.Equals(Environment.GetEnvironmentVariable(EnableFlag), "true", StringComparison.OrdinalIgnoreCase)
        || string.Equals(Environment.GetEnvironmentVariable(EnableFlag), "1", StringComparison.OrdinalIgnoreCase);

    public static SmokeTestConfiguration Load()
    {
        var baseUrl = Environment.GetEnvironmentVariable("CRONIQ_API_BASEURL") ?? "http://localhost:5080";
        var apiKey = Environment.GetEnvironmentVariable("CRONIQ_API_KEY") ?? "smoke-key";
        var tenantId = Environment.GetEnvironmentVariable("CRONIQ_TENANT_ID") ?? "default";
        var environmentTag = Environment.GetEnvironmentVariable("CRONIQ_ENVIRONMENT") ?? "dev";
        var webhookBaseUrl = Environment.GetEnvironmentVariable("CRONIQ_WEBHOOK_BASEURL") ?? baseUrl;

        if (!baseUrl.EndsWith('/'))
        {
            baseUrl += "/";
        }

        if (!webhookBaseUrl.EndsWith('/'))
        {
            webhookBaseUrl += "/";
        }

        return new SmokeTestConfiguration(baseUrl, apiKey, tenantId, environmentTag, webhookBaseUrl);
    }
}
