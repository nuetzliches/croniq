using System.ComponentModel.DataAnnotations;

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

    /// <summary>Base URL of the Croniq server, e.g. <c>http://localhost:4000</c>.</summary>
    [Required, Url]
    public string ServerUrl { get; set; } = "http://localhost:4000";

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
