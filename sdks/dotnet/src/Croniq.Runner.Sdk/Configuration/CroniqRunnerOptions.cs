using System.ComponentModel.DataAnnotations;

using Microsoft.Extensions.Options;

namespace Croniq.Runner.Sdk.Configuration;

/// <summary>
/// Configuration for a single Croniq runner instance. Bind from
/// <c>IConfiguration</c> with <see cref="SectionName"/> as the section path.
/// </summary>
public sealed class CroniqRunnerOptions
{
    /// <summary>Configuration section path: <c>Croniq:Runner</c>.</summary>
    public const string SectionName = "Croniq:Runner";

    /// <summary>Base URL of the Croniq server, e.g. <c>http://localhost:4000</c>.</summary>
    [Required, Url]
    public string ServerUrl { get; set; } = "http://localhost:4000";

    /// <summary>
    /// Stable runner identifier. If <c>null</c>, the SDK resolves it via:
    /// <c>RUNNER_ID</c> environment variable → state file under
    /// <see cref="RunnerDataDir"/> → newly generated <c>{prefix}-{guid8}</c>
    /// (persisted to the state file).
    /// </summary>
    public string? RunnerId { get; set; }

    /// <summary>Prefix used when generating a fresh runner ID.</summary>
    public string RunnerIdPrefix { get; set; } = "runner";

    /// <summary>
    /// Directory the SDK reads/writes the persistent runner-id file in.
    /// Honors the <c>CRONIQ_RUNNER_DATA_DIR</c> environment variable when
    /// <c>null</c>. Defaults to a per-user state directory.
    /// </summary>
    public string? RunnerDataDir { get; set; }

    /// <summary>
    /// API key used for <c>Authorization: ApiKey {key}</c>. Takes precedence
    /// over <see cref="BearerToken"/> when both are set.
    /// </summary>
    public string? ApiKey { get; set; }

    /// <summary>Bearer token used for <c>Authorization: Bearer {token}</c>.</summary>
    public string? BearerToken { get; set; }

    /// <summary>
    /// Capabilities the runner advertises (e.g. <c>"billing"</c>,
    /// <c>"reporting"</c>). Used by the server for job routing
    /// (<c>require</c>/<c>prefer</c> in the Croniqfile).
    /// </summary>
    public IList<string> Capabilities { get; } = [];

    /// <summary>
    /// Free-form tags self-declared by the runner. Filter-only — does
    /// <em>not</em> influence routing (capabilities do that). Convention:
    /// <c>key=value</c> strings (<c>env=prod</c>, <c>lang=dotnet</c>).
    /// </summary>
    public IList<string> Tags { get; } = [];

    /// <summary>Maximum concurrent in-flight executions.</summary>
    [Range(1, 1024)]
    public int MaxInflight { get; set; } = 5;

    /// <summary>Per-request timeout for the long-poll work endpoint.</summary>
    public TimeSpan PollTimeout { get; set; } = TimeSpan.FromSeconds(35);

    /// <summary>
    /// Interval at which the runner sends lease-renewal heartbeats for each
    /// in-flight execution.
    /// </summary>
    public TimeSpan RenewInterval { get; set; } = TimeSpan.FromSeconds(15);

    /// <summary>
    /// Maximum time the runner waits for in-flight executions to finish
    /// during graceful shutdown before returning from <c>RunAsync</c>.
    /// </summary>
    public TimeSpan DrainTimeout { get; set; } = TimeSpan.FromSeconds(30);

    /// <summary>Back-off after a failed poll request.</summary>
    public TimeSpan PollRetryDelay { get; set; } = TimeSpan.FromSeconds(5);

    /// <summary>
    /// Maximum number of consecutive <c>409 Conflict</c> responses from
    /// <c>POST /v1/work/poll</c> before the runner gives up and throws
    /// <see cref="Croniq.Runner.Sdk.PollInstanceConflictException"/>.
    /// Default: 3.
    /// </summary>
    /// <remarks>
    /// A 409 means another process is already registered with the same
    /// <c>runner_id</c>. Retrying forever just masks an operator
    /// misconfiguration. The counter resets on any successful poll or
    /// a non-409 transient error (5xx, network, timeout). See issue
    /// <see href="https://github.com/nuetzliches/croniq/issues/134">#134</see>
    /// sub-item 1.
    /// </remarks>
    [Range(1, 100)]
    public int MaxConsecutivePollConflicts { get; set; } = 3;

    /// <summary>Idle delay when the runner is at <see cref="MaxInflight"/> capacity.</summary>
    public TimeSpan CapacityBackoff { get; set; } = TimeSpan.FromMilliseconds(500);

    /// <summary>Streaming log-writer tunables.</summary>
    public LogWriterOptions LogWriter { get; set; } = new();
}

/// <summary>
/// Source-generated validator for <see cref="CroniqRunnerOptions"/>. The
/// <c>[OptionsValidator]</c> generator emits the DataAnnotation checks at
/// compile time, replacing the reflection-based <c>ValidateDataAnnotations()</c>
/// so the options layer stays trim- and AOT-safe. Registered as an
/// <see cref="IValidateOptions{TOptions}"/> and driven by <c>ValidateOnStart()</c>.
/// </summary>
[OptionsValidator]
internal sealed partial class CroniqRunnerOptionsValidator : IValidateOptions<CroniqRunnerOptions>;
