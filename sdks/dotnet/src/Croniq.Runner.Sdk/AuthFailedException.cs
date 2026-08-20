namespace Croniq.Runner.Sdk;

/// <summary>
/// Thrown from <see cref="CroniqRunner.RunAsync(System.Threading.CancellationToken)"/>
/// when the server returns <c>401 Unauthorized</c> from the poll endpoint
/// <see cref="Croniq.Runner.Sdk.Configuration.CroniqRunnerOptions.MaxConsecutiveAuthFailures"/>
/// times in a row: the API key was rejected and keeps being rejected.
///
/// The credential is read once, when the client is built, and never re-read, so
/// retrying presents the same dead key forever. Before this existed a 401 fell
/// into the generic transient bucket and the runner retried on the poll interval
/// indefinitely: the process stayed up, looked healthy, did nothing, and never
/// exited non-zero — so no supervisor restarted it, and restarting is exactly what
/// would have picked up the new key. Hosts should let this propagate so the
/// process exits non-zero and the dead credential reaches monitoring.
///
/// Not thrown on the first 401. Key rotation hands over by installing the new key
/// and giving the old one an expiry (server issue
/// <see href="https://github.com/nuetzliches/croniq/issues/471">#471</see>), and
/// dying on a single 401 would turn a narrow race around that handover into an
/// outage.
///
/// See issue <see href="https://github.com/nuetzliches/croniq/issues/473">#473</see>.
/// </summary>
public sealed class AuthFailedException : Exception
{
    /// <summary>
    /// The number of consecutive 401 responses observed before bailing.
    /// Equal to <see cref="Croniq.Runner.Sdk.Configuration.CroniqRunnerOptions.MaxConsecutiveAuthFailures"/>
    /// at throw time.
    /// </summary>
    public int ConsecutiveCount { get; }

    public AuthFailedException(int consecutiveCount)
        : base(BuildMessage(consecutiveCount))
    {
        ConsecutiveCount = consecutiveCount;
    }

    public AuthFailedException(int consecutiveCount, Exception innerException)
        : base(BuildMessage(consecutiveCount), innerException)
    {
        ConsecutiveCount = consecutiveCount;
    }

    private static string BuildMessage(int consecutiveCount) =>
        $"unauthorized — the API key was rejected on {consecutiveCount} consecutive "
        + "POST /v1/work/poll attempts. It may have been revoked, or its rotation grace "
        + "window may have elapsed. Restart the runner with the current key.";
}
