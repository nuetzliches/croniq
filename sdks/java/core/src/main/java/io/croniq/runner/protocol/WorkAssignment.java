package io.croniq.runner.protocol;

import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.databind.JsonNode;

/**
 * One work item delivered in a {@link PollResponse}. {@code metadata} is left
 * as an opaque {@link JsonNode} so handlers can navigate it without the SDK
 * needing to know the job-specific schema.
 *
 * <p>{@code timeout} is a humane string (e.g., {@code "1m"}, {@code "30s"}) —
 * parsed by {@link io.croniq.runner.internal.HumanDuration} at dispatch time.
 *
 * <p>{@code scheduledFor} is the original logical fire time (RFC 3339);
 * {@code null} when the server predates the field — consumers must not fall
 * back to {@code fireAt}.
 */
public record WorkAssignment(
        @JsonProperty("execution_id") String executionId,
        @JsonProperty("job_key") String jobKey,
        @JsonProperty("fire_at") String fireAt,
        @JsonProperty("scheduled_for") String scheduledFor,
        @JsonProperty("attempt") int attempt,
        @JsonProperty("metadata") JsonNode metadata,
        @JsonProperty("timeout") String timeout) {}
