using Croniq.Runner.Sdk.Configuration;

using Microsoft.Extensions.Options;

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Adds the <c>Authorization</c> header to every outgoing request based on the
/// current <see cref="CroniqRunnerOptions"/>. <c>ApiKey</c> takes precedence
/// over <c>BearerToken</c> when both are set.
/// </summary>
internal sealed class CroniqAuthHandler(IOptionsMonitor<CroniqRunnerOptions> options) : DelegatingHandler
{
    protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        var current = options.CurrentValue;
        if (!string.IsNullOrEmpty(current.ApiKey))
        {
            request.Headers.TryAddWithoutValidation("Authorization", $"ApiKey {current.ApiKey}");
        }
        else if (!string.IsNullOrEmpty(current.BearerToken))
        {
            request.Headers.TryAddWithoutValidation("Authorization", $"Bearer {current.BearerToken}");
        }
        return base.SendAsync(request, cancellationToken);
    }
}
