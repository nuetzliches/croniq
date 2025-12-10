using System;

namespace Croniq.Api.Smoke;

internal sealed record SmokeTestConfiguration(string BaseUrl, string ApiKey, string TenantId, string EnvironmentTag, string WebhookBaseUrl)
{
    public static SmokeTestConfiguration Load()
    {
        var baseUrl = Environment.GetEnvironmentVariable("CRONIQ_API_BASEURL") ?? "http://localhost:5080";
        var apiKey = Environment.GetEnvironmentVariable("CRONIQ_API_KEY") ?? "smoke-key";
        var tenantId = Environment.GetEnvironmentVariable("CRONIQ_TENANT_ID") ?? "1";
        var environmentTag = Environment.GetEnvironmentVariable("CRONIQ_ENVIRONMENT_TAG") ?? "dev";
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
