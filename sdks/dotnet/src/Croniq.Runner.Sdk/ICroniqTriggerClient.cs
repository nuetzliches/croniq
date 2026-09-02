namespace Croniq.Runner.Sdk;

/// <summary>
/// Producer-side client for firing Croniq jobs on demand via
/// <c>POST /v1/trigger</c>. Register with
/// <c>services.AddCroniqClient(...)</c>; independent of the runner — a pure
/// producer does not need <c>AddCroniqRunner</c>, and the client uses its own
/// credentials (the endpoint requires the <c>jobs:trigger</c> or
/// <c>admin</c> scope, which runner poll keys typically do not carry).
/// </summary>
public interface ICroniqTriggerClient
{
    /// <summary>
    /// Fire a job immediately. The job's registered handler runs on the next
    /// eligible runner, exactly like a scheduled fire.
    /// </summary>
    /// <param name="jobKey">Job key, e.g. <c>billing:invoice-generate</c>.</param>
    /// <param name="metadata">
    /// Metadata passed to the handler. Merged over the job's DSL metadata;
    /// keys starting with <c>__</c> are reserved for internal use.
    /// </param>
    /// <param name="require">
    /// Capabilities a runner must have to be assigned this execution.
    /// <c>null</c> — or empty — inherits the job's <c>runner { require … }</c>.
    /// </param>
    /// <param name="prefer">
    /// Capabilities used to prefer runners when several are eligible.
    /// <c>null</c> or empty inherits the job's <c>runner { prefer … }</c>.
    /// </param>
    /// <param name="timeout">
    /// Execution timeout as a server duration string (e.g. <c>"30s"</c>,
    /// <c>"5m"</c>). <c>null</c> or blank inherits the job's configured
    /// <c>timeout</c>; the server falls back to 5m only when the job declares
    /// none either.
    /// </param>
    /// <param name="idempotencyKey">
    /// Optional dedup key. Servers with trigger-idempotency support coalesce
    /// repeat triggers carrying the same key onto the existing execution
    /// (see <see cref="TriggerResult.Deduplicated"/>); older servers ignore it.
    /// </param>
    /// <param name="cancellationToken">Cancels the HTTP call.</param>
    /// <returns>The created (or deduplicated) execution and queue depth.</returns>
    Task<TriggerResult> TriggerAsync(
        string jobKey,
        IReadOnlyDictionary<string, string>? metadata = null,
        IReadOnlyList<string>? require = null,
        IReadOnlyList<string>? prefer = null,
        string? timeout = null,
        string? idempotencyKey = null,
        CancellationToken cancellationToken = default);
}
