package io.croniq.runner;

/**
 * Result of an on-demand job trigger ({@code POST /v1/trigger}).
 *
 * @param executionId identifier of the execution the trigger resolved to.
 * @param queued server work-queue depth after the trigger was processed.
 * @param deduplicated {@code true} when the server coalesced this trigger onto
 *     an existing execution because the request carried an
 *     {@code idempotency_key} it had already seen; {@link #executionId()} then
 *     refers to that existing execution. Always {@code false} on servers
 *     without idempotency-key support (they omit the flag on the wire).
 * @param coalesced {@code true} when the server folded this trigger into an
 *     execution that was already queued for the job, because the job declares
 *     {@code coalesce}; {@link #executionId()} then refers to that execution
 *     and nothing was enqueued. Distinct from {@code deduplicated} even though
 *     both mean "no new execution": a dedup hit can name an execution that
 *     started <em>before</em> this call, while a fold only ever names one that
 *     starts after it. Always {@code false} on servers without
 *     {@code coalesce} support.
 */
public record TriggerResult(String executionId, int queued, boolean deduplicated, boolean coalesced) {}
