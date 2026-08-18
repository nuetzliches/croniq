using Microsoft.Extensions.Logging;
using Microsoft.Extensions.Options;

namespace Croniq.Runner.Sdk.Configuration;

/// <summary>
/// Base class for the two <c>ServerUrl</c> transport-security validators.
/// <para>
/// Registered alongside the source-generated DataAnnotations validators, so a
/// cleartext base URL is refused when the options are first materialised —
/// which <c>ValidateOnStart()</c> forces at host startup. That is deliberately
/// earlier than the first request: the whole point is that a misconfigured
/// deployment fails fast instead of leaking the API key on every poll.
/// </para>
/// <para>
/// The opt-in warning goes through <see cref="ILogger"/> rather than the
/// validation result because it must NOT fail startup — it has to be seen, and
/// a validator is the one place that reliably runs once, at startup, with the
/// final option values in hand.
/// </para>
/// </summary>
internal abstract class ServerUrlSecurityValidator<TOptions>(ILoggerFactory loggerFactory)
    : IValidateOptions<TOptions>
    where TOptions : class
{
    private readonly ILogger _logger = loggerFactory.CreateLogger("Croniq.Runner.Sdk.Security");
    private int _warned;

    /// <summary>Reads the configured base URL off the options instance.</summary>
    protected abstract string? GetServerUrl(TOptions options);

    /// <summary>Reads the cleartext-HTTP opt-in off the options instance.</summary>
    protected abstract bool GetAllowInsecureHttp(TOptions options);

    /// <summary>Options type name used in messages.</summary>
    protected abstract string OptionsName { get; }

    public ValidateOptionsResult Validate(string? name, TOptions options)
    {
        ArgumentNullException.ThrowIfNull(options);

        var result = ServerUrlSecurity.Check(
            GetServerUrl(options),
            GetAllowInsecureHttp(options),
            OptionsName);

        if (result.Error is not null)
        {
            return ValidateOptionsResult.Fail(result.Error);
        }

        // One loud warning per registration, however often the options are
        // re-materialised (named options, reloads, IOptionsMonitor).
        if (result.Warning is not null && Interlocked.Exchange(ref _warned, 1) == 0)
        {
#pragma warning disable CA2254 // Template is a constant per call site; the URL is interpolated in deliberately.
            _logger.LogWarning(result.Warning);
#pragma warning restore CA2254
        }

        return ValidateOptionsResult.Success;
    }
}

/// <summary>
/// Refuses a cleartext <see cref="CroniqRunnerOptions.ServerUrl"/> on a
/// non-loopback host unless <see cref="CroniqRunnerOptions.AllowInsecureHttp"/>
/// is set.
/// </summary>
internal sealed class CroniqRunnerOptionsSecurityValidator(ILoggerFactory loggerFactory)
    : ServerUrlSecurityValidator<CroniqRunnerOptions>(loggerFactory)
{
    protected override string OptionsName => nameof(CroniqRunnerOptions);

    protected override string? GetServerUrl(CroniqRunnerOptions options) => options.ServerUrl;

    protected override bool GetAllowInsecureHttp(CroniqRunnerOptions options) => options.AllowInsecureHttp;
}

/// <summary>
/// Refuses a cleartext <see cref="CroniqClientOptions.ServerUrl"/> on a
/// non-loopback host unless <see cref="CroniqClientOptions.AllowInsecureHttp"/>
/// is set.
/// </summary>
internal sealed class CroniqClientOptionsSecurityValidator(ILoggerFactory loggerFactory)
    : ServerUrlSecurityValidator<CroniqClientOptions>(loggerFactory)
{
    protected override string OptionsName => nameof(CroniqClientOptions);

    protected override string? GetServerUrl(CroniqClientOptions options) => options.ServerUrl;

    protected override bool GetAllowInsecureHttp(CroniqClientOptions options) => options.AllowInsecureHttp;
}
