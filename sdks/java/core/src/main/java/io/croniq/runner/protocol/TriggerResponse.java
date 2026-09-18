package io.croniq.runner.protocol;

import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * Wire response of {@code POST /v1/trigger}.
 *
 * <p>{@code deduplicated} is sent by servers that support trigger idempotency
 * keys and {@code coalesced} by servers that support the {@code coalesce} job
 * directive; older servers omit either, and Jackson leaves the primitive at its
 * default ({@code false}) — the forward-compatible parse contract the
 * conformance suite pins.
 */
public record TriggerResponse(
        @JsonProperty("execution_id") String executionId,
        @JsonProperty("queued") int queued,
        @JsonProperty("deduplicated") boolean deduplicated,
        @JsonProperty("coalesced") boolean coalesced) {}
