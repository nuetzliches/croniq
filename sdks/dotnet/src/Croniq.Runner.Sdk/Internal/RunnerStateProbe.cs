namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Snapshot of runner liveness state for diagnostics. Consumed by the
/// health check and any caller that wants to introspect the runner.
/// </summary>
internal interface IRunnerStateProbe
{
    DateTimeOffset? LastSuccessfulPollAt { get; }
    DateTimeOffset? LastPollFailureAt { get; }
    string? LastPollError { get; }
    int InflightCount { get; }
    bool HasStarted { get; }
    bool IsDraining { get; }
}

internal sealed class RunnerStateProbe : IRunnerStateProbe
{
    private DateTimeOffset? _lastSuccessfulPollAt;
    private DateTimeOffset? _lastPollFailureAt;
    private string? _lastPollError;
    private int _inflight;
    private bool _hasStarted;
    private bool _isDraining;

    public DateTimeOffset? LastSuccessfulPollAt => _lastSuccessfulPollAt;
    public DateTimeOffset? LastPollFailureAt => _lastPollFailureAt;
    public string? LastPollError => _lastPollError;
    public int InflightCount => Volatile.Read(ref _inflight);
    public bool HasStarted => _hasStarted;
    public bool IsDraining => _isDraining;

    public void MarkStarted() => _hasStarted = true;
    public void MarkDraining() => _isDraining = true;

    public void MarkSuccessfulPoll(DateTimeOffset at)
    {
        _lastSuccessfulPollAt = at;
        _lastPollError = null;
    }

    public void MarkPollFailure(DateTimeOffset at, string error)
    {
        _lastPollFailureAt = at;
        _lastPollError = error;
    }

    public void IncrementInflight() => Interlocked.Increment(ref _inflight);
    public void DecrementInflight() => Interlocked.Decrement(ref _inflight);
}
