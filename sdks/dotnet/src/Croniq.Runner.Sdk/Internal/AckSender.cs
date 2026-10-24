using System.Net;

using Croniq.Runner.Sdk.Protocol;

using Microsoft.Extensions.Logging;

#pragma warning disable CA1031 // every ack failure is logged, never propagated

namespace Croniq.Runner.Sdk.Internal;

/// <summary>
/// Sends <c>POST /v1/work/ack</c> with a short, bounded retry.
/// </summary>
/// <remarks>
/// <para>Review 2026-10-24, K-1: a single failed ack used to cost a full second
/// run of a job that had already succeeded. The dispatcher logged "failed to
/// ack", dropped the execution from the in-flight set, the next poll reported it
/// missing, and the server's stale-claim reaper requeued it 60 s later. The ack
/// is idempotent on the server (CAS-fenced), so resending it is safe.</para>
/// <para>Three attempts in total, about 1 s and then 2 s apart — the same budget
/// in every SDK, pinned by conformance cases 23–25. Only a transport error
/// (including the per-request timeout) or a 5xx is retried. A 4xx is the
/// server's verdict on this ack (409: already terminal or not ours, 404: gone,
/// 403: credential does not own the runner_id); sending it again cannot change
/// the answer. Each attempt gets its own <see cref="Configuration.CroniqRunnerOptions.RequestTimeout"/>
/// from the client, and nothing here observes the runner's shutdown token: the
/// final ack of a drained execution must still go out (K-4).</para>
/// </remarks>
internal static class AckSender
{
    /// <summary>Delays before attempt 2 and attempt 3.</summary>
    internal static readonly IReadOnlyList<TimeSpan> DefaultRetryDelays =
        [TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(2)];

    /// <summary>Upper bound on the time spent in retry delays.</summary>
    internal static TimeSpan TotalRetryDelay(IReadOnlyList<TimeSpan> delays)
    {
        var total = TimeSpan.Zero;
        foreach (var d in delays)
        {
            total += d;
        }
        return total;
    }

    /// <summary>
    /// Whether an ack failure is worth another attempt: no response at all
    /// (connection refused/reset, DNS, request timeout) or a 5xx.
    /// </summary>
    internal static bool IsRetryable(Exception ex) => ex switch
    {
        HttpRequestException { StatusCode: { } status } => (int)status >= 500,
        HttpRequestException => true,
        OperationCanceledException => true, // the per-request timeout; nothing else cancels an ack
        IOException => true,
        _ => false,
    };

    /// <summary>
    /// Send the ack, retrying per the rules above. Never throws; the final
    /// failure is logged at error level.
    /// </summary>
    /// <returns><c>true</c> when the server accepted the ack.</returns>
    internal static async Task<bool> SendAsync(
        ICroniqClient client,
        AckRequest request,
        ILogger logger,
        IReadOnlyList<TimeSpan>? retryDelays = null)
    {
        var delays = retryDelays ?? DefaultRetryDelays;
        var maxAttempts = delays.Count + 1;
        for (var attempt = 1; ; attempt++)
        {
            try
            {
                await client.AckAsync(request, CancellationToken.None).ConfigureAwait(false);
                return true;
            }
            catch (HttpRequestException ex) when (ex.StatusCode == HttpStatusCode.Forbidden)
            {
                // Ownership refusal (#436/#437) — permanent, and it means the
                // execution stays claimed until its lease expires. Distinct
                // message so the operator sees the fix, not just "ack failed".
                logger.LogError(
                    ex,
                    "ack refused with 403 Forbidden — this runner's credential does not own " +
                    "runner_id={RunnerId}, so the execution stays claimed until its lease expires. " +
                    "Give the runner its own runner_id, or release the existing binding with " +
                    "DELETE /v1/runners/{{id}}.",
                    request.RunnerId);
                return false;
            }
            catch (Exception ex)
            {
                if (attempt < maxAttempts && IsRetryable(ex))
                {
                    var delay = delays[attempt - 1];
                    logger.LogWarning(
                        ex,
                        "ack attempt {Attempt}/{MaxAttempts} failed — retrying in {Delay}",
                        attempt, maxAttempts, delay);
                    await Task.Delay(delay, CancellationToken.None).ConfigureAwait(false);
                    continue;
                }
                logger.LogError(
                    ex,
                    "failed to ack execution after {Attempt} attempt(s)",
                    attempt);
                return false;
            }
        }
    }
}
