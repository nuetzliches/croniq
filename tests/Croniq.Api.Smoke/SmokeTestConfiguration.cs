using System;

namespace Croniq.Api.Smoke;

internal sealed record SmokeTestConfiguration(string BaseUrl, string ApiKey)
{
    public static SmokeTestConfiguration Load()
    {
        var baseUrl = Environment.GetEnvironmentVariable("CRONIQ_API_BASEURL") ?? "http://localhost:5080";
        var apiKey = Environment.GetEnvironmentVariable("CRONIQ_API_KEY") ?? "smoke-key";

        if (!baseUrl.EndsWith('/'))
        {
            baseUrl += "/";
        }

        return new SmokeTestConfiguration(baseUrl, apiKey);
    }
}
