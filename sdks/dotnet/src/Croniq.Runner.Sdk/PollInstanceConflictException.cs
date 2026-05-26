namespace Croniq.Runner.Sdk;

/// <summary>
/// Thrown from <see cref="CroniqRunner.RunAsync(System.Threading.CancellationToken)"/>
/// when the server returns <c>409 Conflict</c> from the poll endpoint
/// <see cref="Croniq.Runner.Sdk.Configuration.CroniqRunnerOptions.MaxConsecutivePollConflicts"/>
/// times in a row. A 409 on poll means another runner process is
/// already registered with the same <c>runner_id</c>; retrying forever
/// just masks an operator misconfiguration. Hosts should let this
/// propagate so the process exits with a non-zero status code,
/// surfacing the issue to monitoring instead of silently looping.
///
/// See issue <see href="https://github.com/nuetzliches/croniq/issues/134">#134</see>
/// sub-item 1.
/// </summary>
public sealed class PollInstanceConflictException : Exception
{
    /// <summary>
    /// The runner identifier that triggered the conflict. Helpful for
    /// log correlation: operators can grep `runner_id=<value>` in audit
    /// logs to find the duplicate.
    /// </summary>
    public string RunnerId { get; }

    /// <summary>
    /// The number of consecutive 409 responses observed before bailing.
    /// Equal to <see cref="Croniq.Runner.Sdk.Configuration.CroniqRunnerOptions.MaxConsecutivePollConflicts"/>
    /// at throw time.
    /// </summary>
    public int ConsecutiveCount { get; }

    public PollInstanceConflictException(string runnerId, int consecutiveCount)
        : base(
            $"poll instance conflict — another runner is already registered with runner_id '{runnerId}'. "
            + $"Observed {consecutiveCount} consecutive 409 Conflict responses on POST /v1/work/poll. "
            + "Stop the duplicate process or rotate the runner_id.")
    {
        RunnerId = runnerId;
        ConsecutiveCount = consecutiveCount;
    }

    public PollInstanceConflictException(string runnerId, int consecutiveCount, Exception innerException)
        : base(
            $"poll instance conflict — another runner is already registered with runner_id '{runnerId}'. "
            + $"Observed {consecutiveCount} consecutive 409 Conflict responses on POST /v1/work/poll. "
            + "Stop the duplicate process or rotate the runner_id.",
            innerException)
    {
        RunnerId = runnerId;
        ConsecutiveCount = consecutiveCount;
    }
}
