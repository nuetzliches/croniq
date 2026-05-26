package io.croniq.runner.protocol;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;
import java.util.Map;

/**
 * One log event streamed via POST /v1/work/{execution_id}/events.
 *
 * <p>The {@code fields} map carries caller-defined structured context.
 * Standard SDK-injected keys ({@code job_key}, {@code runner_id},
 * {@code runner_tags}) are added by the LogWriter in PR-4 — caller-supplied
 * values for those keys take precedence.
 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record WorkEvent(
        @JsonProperty("level") String level,
        @JsonProperty("message") String message,
        @JsonProperty("fields") Map<String, String> fields) {}
