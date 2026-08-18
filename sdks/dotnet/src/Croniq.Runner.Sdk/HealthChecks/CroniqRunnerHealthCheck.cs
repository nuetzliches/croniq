using Croniq.Runner.Sdk.Internal;

using Microsoft.Extensions.Diagnostics.HealthChecks;

namespace Croniq.Runner.Sdk.HealthChecks;

/// <summary>
/// Health-check that reports the liveness of the Croniq runner based on
/// the most recent successful poll timestamp.
/// <list type="bullet">
///   <item><description><see cref="HealthStatus.Healthy"/> — last successful poll within <see cref="HealthyThreshold"/>.</description></item>
///   <item><description><see cref="HealthStatus.Degraded"/> — within <see cref="UnhealthyThreshold"/> but past <see cref="HealthyThreshold"/>.</description></item>
///   <item><description><see cref="HealthStatus.Unhealthy"/> — past <see cref="UnhealthyThreshold"/>, never started, or last error recorded.</description></item>
/// </list>
/// <para><b>The description is public.</b> Health endpoints are commonly
/// exposed without authentication, and a custom or dashboard response writer
/// renders the description verbatim. Everything written here is therefore
/// either a fixed literal, a duration, or the fixed failure category from
/// <c>CroniqRunner.DescribePollFailure</c> — never raw exception text, which
/// would leak the resolved Croniq host and port to an anonymous reader.</para>
/// </summary>
public sealed class CroniqRunnerHealthCheck : IHealthCheck
{
    private readonly IRunnerStateProbe _probe;
    private readonly TimeProvider _timeProvider;

    internal CroniqRunnerHealthCheck(IRunnerStateProbe probe, TimeProvider? timeProvider = null)
    {
        _probe = probe;
        _timeProvider = timeProvider ?? TimeProvider.System;
    }
    /// <summary>How recently we need a successful poll to consider the runner healthy.</summary>
    public TimeSpan HealthyThreshold { get; set; } = TimeSpan.FromMinutes(1);

    /// <summary>Beyond this, the runner is unhealthy.</summary>
    public TimeSpan UnhealthyThreshold { get; set; } = TimeSpan.FromMinutes(5);

    public Task<HealthCheckResult> CheckHealthAsync(HealthCheckContext context, CancellationToken cancellationToken = default)
    {
        if (!_probe.HasStarted)
        {
            return Task.FromResult(HealthCheckResult.Degraded("Croniq runner has not started yet"));
        }

        if (_probe.IsDraining)
        {
            return Task.FromResult(HealthCheckResult.Degraded("Croniq runner is draining"));
        }

        var now = _timeProvider.GetUtcNow();
        var last = _probe.LastSuccessfulPollAt;
        if (last is null)
        {
            return Task.FromResult(HealthCheckResult.Unhealthy("Croniq runner has not completed a successful poll yet"));
        }

        var since = now - last.Value;
        if (since <= HealthyThreshold)
        {
            return Task.FromResult(HealthCheckResult.Healthy(
                $"last poll {since.TotalSeconds:0}s ago",
                new Dictionary<string, object> { ["last_poll_at"] = last.Value, ["inflight"] = _probe.InflightCount }));
        }
        if (since <= UnhealthyThreshold)
        {
            return Task.FromResult(HealthCheckResult.Degraded(
                $"last poll {since.TotalSeconds:0}s ago (reason: {_probe.LastPollFailureReason ?? "n/a"})"));
        }
        return Task.FromResult(HealthCheckResult.Unhealthy(
            $"no successful poll for {since.TotalSeconds:0}s (reason: {_probe.LastPollFailureReason ?? "n/a"})"));
    }
}
