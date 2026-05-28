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
        string? value = null;
        if (!string.IsNullOrEmpty(current.ApiKey))
        {
            value = $"ApiKey {current.ApiKey}";
        }
        else if (!string.IsNullOrEmpty(current.BearerToken))
        {
            value = $"Bearer {current.BearerToken}";
        }

        if (value is not null)
        {
            // Belt-and-braces against duplicate handler registrations or any
            // upstream code that already wrote an Authorization header:
            // TryAddWithoutValidation does NOT replace, it appends — which
            // would produce a comma-joined header the server can't split
            // back into a valid credential.
            request.Headers.Remove("Authorization");
            request.Headers.TryAddWithoutValidation("Authorization", value);
        }

        return base.SendAsync(request, cancellationToken);
    }
}
