using System.ComponentModel.DataAnnotations;

using Microsoft.Extensions.Options;

namespace Croniq.Runner.Sdk.Configuration;

/// <summary>
/// Configuration for the producer-side trigger client
/// (<see cref="Croniq.Runner.Sdk.ICroniqTriggerClient"/>). Bind from
/// <c>IConfiguration</c> with <see cref="SectionName"/> as the section path.
/// <para>
/// Deliberately separate from <see cref="CroniqRunnerOptions"/>: triggering
/// requires the <c>jobs:trigger</c> (or <c>admin</c>) scope, which is
/// distinct from the runner's poll scopes — the trigger client therefore
/// carries its own credentials instead of assuming the runner's.
/// </para>
/// </summary>
public sealed class CroniqClientOptions
{
    /// <summary>Configuration section path: <c>Croniq:Client</c>.</summary>
    public const string SectionName = "Croniq:Client";

    /// <summary>
    /// Base URL of the Croniq server, e.g. <c>http://localhost:4000</c>.
    /// <para>
    /// <c>https://</c> is required unless the host is loopback
    /// (<c>localhost</c>, <c>127.0.0.0/8</c>, <c>::1</c>) — the trigger
    /// credential rides along on every request and would otherwise travel in
    /// cleartext. See <see cref="AllowInsecureHttp"/>.
    /// </para>
    /// </summary>
    [Required, Url]
    public string ServerUrl { get; set; } = "http://localhost:4000";

    /// <summary>
    /// Opt in to a cleartext <c>http://</c> <see cref="ServerUrl"/> on a
    /// non-loopback host. Off by default: such a URL otherwise fails options
    /// validation at startup. With it the client works but logs one loud
    /// warning — the credential then travels in cleartext on every trigger
    /// call. Lab and staging only; never production.
    /// </summary>
    public bool AllowInsecureHttp { get; set; }

    /// <summary>
    /// API key used for <c>Authorization: ApiKey {key}</c>. Takes precedence
    /// over <see cref="BearerToken"/> when both are set. Needs the
    /// <c>jobs:trigger</c> or <c>admin</c> scope.
    /// </summary>
    public string? ApiKey { get; set; }

    /// <summary>Bearer token used for <c>Authorization: Bearer {token}</c>.</summary>
    public string? BearerToken { get; set; }

    /// <summary>Per-request timeout for trigger calls.</summary>
    public TimeSpan RequestTimeout { get; set; } = TimeSpan.FromSeconds(30);
}

/// <summary>
/// Source-generated validator for <see cref="CroniqClientOptions"/>. See
/// <see cref="CroniqRunnerOptionsValidator"/> — same rationale: the
/// <c>[OptionsValidator]</c> generator replaces reflection-based
/// <c>ValidateDataAnnotations()</c> to keep the options layer trim/AOT-safe.
/// </summary>
[OptionsValidator]
internal sealed partial class CroniqClientOptionsValidator : IValidateOptions<CroniqClientOptions>;
