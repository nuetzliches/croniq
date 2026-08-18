namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Snapshot of runner liveness state for diagnostics. Consumed by the
/// health check and any caller that wants to introspect the runner.
/// </summary>
internal interface IRunnerStateProbe
{
    DateTimeOffset? LastSuccessfulPollAt { get; }
    DateTimeOffset? LastPollFailureAt { get; }

    /// <summary>
    /// Fixed category describing why the last poll failed — never raw
    /// exception text. Rendered into the (frequently unauthenticated)
    /// health-check description, so it must not carry hostnames, ports, URLs
    /// or any other deployment detail. Produced by
    /// <c>CroniqRunner.DescribePollFailure</c>.
    /// </summary>
    string? LastPollFailureReason { get; }
    int InflightCount { get; }
    bool HasStarted { get; }
    bool IsDraining { get; }
}

internal sealed class RunnerStateProbe : IRunnerStateProbe
{
    private DateTimeOffset? _lastSuccessfulPollAt;
    private DateTimeOffset? _lastPollFailureAt;
    private string? _lastPollFailureReason;
    private int _inflight;
    private bool _hasStarted;
    private bool _isDraining;

    public DateTimeOffset? LastSuccessfulPollAt => _lastSuccessfulPollAt;
    public DateTimeOffset? LastPollFailureAt => _lastPollFailureAt;
    public string? LastPollFailureReason => _lastPollFailureReason;
    public int InflightCount => Volatile.Read(ref _inflight);
    public bool HasStarted => _hasStarted;
    public bool IsDraining => _isDraining;

    public void MarkStarted() => _hasStarted = true;
    public void MarkDraining() => _isDraining = true;

    public void MarkSuccessfulPoll(DateTimeOffset at)
    {
        _lastSuccessfulPollAt = at;
        _lastPollFailureReason = null;
    }

    /// <param name="at">When the failure was observed.</param>
    /// <param name="reason">
    /// A fixed category from <c>CroniqRunner.DescribePollFailure</c>. Never
    /// pass <c>ex.Message</c> — this value reaches the public health-check
    /// description.
    /// </param>
    public void MarkPollFailure(DateTimeOffset at, string reason)
    {
        _lastPollFailureAt = at;
        _lastPollFailureReason = reason;
    }

    public void IncrementInflight() => Interlocked.Increment(ref _inflight);
    public void DecrementInflight() => Interlocked.Decrement(ref _inflight);
}
