namespace Croniq.Runner.Sdk;

/// <summary>
/// Result of an on-demand job trigger (<c>POST /v1/trigger</c>).
/// </summary>
/// <param name="ExecutionId">Identifier of the execution the trigger resolved to.</param>
/// <param name="Queued">Server work-queue depth after the trigger was processed.</param>
/// <param name="Deduplicated">
/// <c>true</c> when the server coalesced this trigger onto an existing
/// execution because the request carried an <c>idempotency_key</c> it had
/// already seen. <see cref="ExecutionId"/> then refers to that existing
/// execution. Always <c>false</c> on servers without idempotency-key support.
/// </param>
/// <param name="Coalesced">
/// <c>true</c> when the server folded this trigger into an execution that was
/// already queued for the job, because the job declares <c>coalesce</c>.
/// <see cref="ExecutionId"/> then refers to that execution and nothing was
/// enqueued. Distinct from <see cref="Deduplicated"/> even though both mean
/// "no new execution": a dedup hit can name an execution that started
/// <em>before</em> this call, while a fold only ever names one that starts
/// after it. Always <c>false</c> on servers without <c>coalesce</c> support.
/// </param>
public sealed record TriggerResult(
    string ExecutionId,
    int Queued,
    bool Deduplicated = false,
    bool Coalesced = false);
