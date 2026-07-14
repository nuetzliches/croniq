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
 */
public record TriggerResult(String executionId, int queued, boolean deduplicated) {}
