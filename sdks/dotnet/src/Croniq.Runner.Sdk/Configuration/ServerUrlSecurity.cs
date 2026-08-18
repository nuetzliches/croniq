using System.Net;

namespace Croniq.Runner.Sdk.Configuration;

/// <summary>
/// Transport-security checks applied to a configured <c>ServerUrl</c>.
/// <para>
/// Both the runner and the producer-side trigger client attach the credential
/// as an <c>Authorization</c> header on every request. Over <c>http://</c> that
/// key travels in cleartext — and through any HTTP proxy the environment
/// configures. The rule (identical in the Java, Python, Go and TypeScript
/// SDKs): <c>https://</c> is always fine, <c>http://</c> is fine for a loopback
/// host — that is the <c>http://localhost:4000</c> quickstart path — and
/// <c>http://</c> against any other host is refused unless the caller
/// explicitly sets <c>AllowInsecureHttp</c>.
/// </para>
/// </summary>
internal static class ServerUrlSecurity
{
    /// <summary>
    /// Outcome of a base-URL check: at most one of <see cref="Error"/> (the
    /// configuration is refused) and <see cref="Warning"/> (accepted under an
    /// explicit opt-in, but the credential is exposed) is non-<c>null</c>.
    /// </summary>
    internal readonly record struct Result(string? Error, string? Warning)
    {
        internal static Result Ok { get; } = new(null, null);
    }

    /// <summary>
    /// <c>true</c> for the loopback hosts considered safe over cleartext HTTP:
    /// <c>localhost</c>, anything in <c>127.0.0.0/8</c>, and IPv6 <c>::1</c>
    /// (bare or in the bracketed <c>[::1]</c> form <see cref="Uri.Host"/>
    /// produces).
    /// </summary>
    internal static bool IsLoopbackHost(string? host)
    {
        if (string.IsNullOrWhiteSpace(host))
        {
            return false;
        }

        var candidate = host.Trim().Trim('[', ']');
        if (candidate.Equals("localhost", StringComparison.OrdinalIgnoreCase))
        {
            return true;
        }

        return IPAddress.TryParse(candidate, out var address) && IPAddress.IsLoopback(address);
    }

    /// <summary>
    /// Validates a configured base URL. Called from the options validators so a
    /// misconfiguration fails at startup rather than on the first request.
    /// </summary>
    /// <param name="serverUrl">The configured base URL.</param>
    /// <param name="allowInsecureHttp">The caller's explicit opt-in to cleartext HTTP.</param>
    /// <param name="optionsName">Options type name, used to make messages actionable.</param>
    internal static Result Check(string? serverUrl, bool allowInsecureHttp, string optionsName)
    {
        if (string.IsNullOrWhiteSpace(serverUrl))
        {
            // [Required] on the property already reports this; nothing to add.
            return Result.Ok;
        }

        if (!Uri.TryCreate(serverUrl, UriKind.Absolute, out var uri))
        {
            // [Url] on the property already reports this; nothing to add.
            return Result.Ok;
        }

        if (uri.Scheme.Equals(Uri.UriSchemeHttps, StringComparison.OrdinalIgnoreCase))
        {
            return Result.Ok;
        }

        if (!uri.Scheme.Equals(Uri.UriSchemeHttp, StringComparison.OrdinalIgnoreCase))
        {
            return new Result(
                $"{optionsName}.ServerUrl '{serverUrl}' has unsupported scheme '{uri.Scheme}': " +
                "use https:// (or http:// for a loopback host).",
                null);
        }

        if (IsLoopbackHost(uri.Host))
        {
            return Result.Ok;
        }

        if (!allowInsecureHttp)
        {
            return new Result(
                $"{optionsName}.ServerUrl '{serverUrl}' uses cleartext http:// with the " +
                $"non-loopback host '{uri.Host}': the API key would be sent in the clear on " +
                "every request, and through any configured HTTP proxy. Use https://, or set " +
                $"{optionsName}.AllowInsecureHttp = true to accept that risk explicitly.",
                null);
        }

        return new Result(
            null,
            $"SECURITY: Croniq is configured against the cleartext URL '{serverUrl}' with " +
            $"{optionsName}.AllowInsecureHttp = true. The API key is transmitted in cleartext " +
            "on every request and is readable by anyone on the network path (including HTTP " +
            "proxies). Use https:// in production.");
    }
}
