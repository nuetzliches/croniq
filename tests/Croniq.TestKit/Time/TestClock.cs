using System;

namespace Croniq.TestKit.Time;

/// <summary>
/// Deterministic clock that allows tests to control <see cref="DateTimeOffset.UtcNow"/> semantics.
/// </summary>
public sealed class TestClock
{
    private DateTimeOffset _current;

    public TestClock(DateTimeOffset? startAtUtc = null)
    {
        _current = startAtUtc ?? DateTimeOffset.UtcNow;
    }

    /// <summary>
    /// Gets the current simulated UTC timestamp.
    /// </summary>
    public DateTimeOffset UtcNow => _current;

    /// <summary>
    /// Advances the clock by the specified delta and returns the new timestamp.
    /// </summary>
    public DateTimeOffset Advance(TimeSpan delta)
    {
        _current = _current.Add(delta);
        return _current;
    }

    /// <summary>
    /// Sets the current timestamp to an explicit value.
    /// </summary>
    public void Set(DateTimeOffset value)
    {
        _current = value;
    }

    /// <summary>
    /// Provides a delegate that can be passed into APIs expecting a time provider.
    /// </summary>
    public Func<DateTimeOffset> AsProvider() => () => _current;
}
