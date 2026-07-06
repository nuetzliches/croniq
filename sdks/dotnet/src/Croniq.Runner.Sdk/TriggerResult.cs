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
public sealed record TriggerResult(string ExecutionId, int Queued, bool Deduplicated = false);
