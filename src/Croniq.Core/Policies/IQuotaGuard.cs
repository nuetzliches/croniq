using System;
using Croniq.Core.Jobs;

namespace Croniq.Core.Policies;

/// <summary>
/// In-memory quota guard to enforce per-job rate and concurrency limits.
/// </summary>
public interface IQuotaGuard
{
    /// <summary>
    /// Attempts to reserve quota for the given job at the specified time.
    /// </summary>
    /// <param name="jobKey">JobKey being executed.</param>
    /// <param name="options">Resolved quota options.</param>
    /// <param name="now">Current timestamp (UTC).</param>
    /// <param name="retryAtUtc">If not allowed, suggested next time to retry.</param>
    /// <returns>True when the execution is permitted.</returns>
    bool TryAcquire(JobKey jobKey, QuotaOptions options, DateTimeOffset now, out DateTimeOffset? retryAtUtc);

    /// <summary>
    /// Releases previously acquired quota reservation.
    /// </summary>
    void Release(JobKey jobKey);
}
