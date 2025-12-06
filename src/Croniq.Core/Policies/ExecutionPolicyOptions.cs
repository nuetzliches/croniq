using System;
using System.Collections.Generic;

namespace Croniq.Core.Policies;

/// <summary>
/// Options controlling retry/timeout/circuit/dead-letter behavior per job.
/// </summary>
public sealed class ExecutionPolicyOptions
{
    public RetryPolicyOptions Retry { get; set; } = new();

    public TimeoutPolicyOptions Timeout { get; set; } = new();

    public CircuitBreakerPolicyOptions CircuitBreaker { get; set; } = new();

    public DeadLetterPolicyOptions DeadLetter { get; set; } = new();
}

public sealed class RetryPolicyOptions
{
    public bool Enabled { get; set; } = true;

    /// <summary>Total number of attempts including the initial execution.</summary>
    public int MaxAttempts { get; set; } = 3;

    public RetryBackoffStrategy BackoffStrategy { get; set; } = RetryBackoffStrategy.Exponential;

    /// <summary>Initial delay before the first retry.</summary>
    public TimeSpan InitialDelay { get; set; } = TimeSpan.FromSeconds(2);

    /// <summary>Maximum delay between retries (used for exponential strategy).</summary>
    public TimeSpan MaxDelay { get; set; } = TimeSpan.FromSeconds(30);

    /// <summary>Optional jitter factor (0-1) applied to delays.</summary>
    public double JitterFactor { get; set; } = 0.25d;

    /// <summary>Fully qualified exception type names that are eligible for retries; empty ⇒ retry all.</summary>
    public IReadOnlyCollection<string> RetryableExceptions { get; set; } = Array.Empty<string>();
}

public enum RetryBackoffStrategy
{
    Fixed,
    Linear,
    Exponential
}

public sealed class TimeoutPolicyOptions
{
    public bool Enabled { get; set; } = true;

    public TimeSpan Timeout { get; set; } = TimeSpan.FromMinutes(5);

    /// <summary>Whether to cancel the job handler when the timeout elapses.</summary>
    public bool CancelExecutionOnTimeout { get; set; } = true;
}

public sealed class CircuitBreakerPolicyOptions
{
    public bool Enabled { get; set; } = false;

    public int FailureThreshold { get; set; } = 5;

    public TimeSpan SamplingWindow { get; set; } = TimeSpan.FromMinutes(1);

    public TimeSpan BreakDuration { get; set; } = TimeSpan.FromMinutes(2);

    public int MinimumThroughput { get; set; } = 20;
}

public sealed class DeadLetterPolicyOptions
{
    public bool Enabled { get; set; } = true;

    public TimeSpan Retention { get; set; } = TimeSpan.FromDays(30);

    /// <summary>Optional hint presented to operators when a message is dead-lettered.</summary>
    public string? OperatorHint { get; set; }
}
