namespace Croniq.Runner.Sdk;

/// <summary>
/// Thrown from <see cref="CroniqRunner.RunAsync(System.Threading.CancellationToken)"/>
/// when a work endpoint answers <c>403 Forbidden</c>: the authenticated
/// credential is bound to a different <c>runner_id</c> than the one this
/// runner names in its requests (server issue
/// <see href="https://github.com/nuetzliches/croniq/issues/436">#436</see>).
///
/// <para>Unlike a <c>409 Conflict</c> — where a duplicate deployment may
/// disappear on its own — a 403 is <em>permanent</em>: no number of retries
/// can clear it. The runner therefore bails on the first occurrence
/// instead of polling forever and looking merely idle. An operator has to
/// give this runner its own <c>runner_id</c>, or release the existing
/// binding with <c>DELETE /v1/runners/{id}</c>.</para>
///
/// <para>Hosts should let this propagate so the process exits non-zero and
/// the misconfiguration reaches monitoring. See issue
/// <see href="https://github.com/nuetzliches/croniq/issues/437">#437</see>.</para>
/// </summary>
public sealed class RunnerOwnershipDeniedException : Exception
{
    /// <summary>
    /// The <c>runner_id</c> the credential was refused for. Operators can
    /// grep <c>runner_id=&lt;value&gt;</c> in the server's audit log to find
    /// the credential that actually owns it.
    /// </summary>
    public string RunnerId { get; }

    private static string BuildMessage(string runnerId) =>
        $"work ownership denied — the credential this runner authenticates with does not own "
        + $"runner_id '{runnerId}'. The server answered 403 Forbidden on POST /v1/work/poll and "
        + "will keep doing so: give this runner its own runner_id, or release the existing "
        + "binding with DELETE /v1/runners/{id}.";

    public RunnerOwnershipDeniedException(string runnerId)
        : base(BuildMessage(runnerId))
    {
        RunnerId = runnerId;
    }

    public RunnerOwnershipDeniedException(string runnerId, Exception innerException)
        : base(BuildMessage(runnerId), innerException)
    {
        RunnerId = runnerId;
    }
}
