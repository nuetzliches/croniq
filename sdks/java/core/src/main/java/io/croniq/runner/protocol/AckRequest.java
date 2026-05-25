package io.croniq.runner.protocol;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;

/**
 * POST /v1/work/ack request body. {@code status} is the string {@code "success"}
 * or {@code "failure"} — see {@link Status} for the canonical constants.
 */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record AckRequest(
        @JsonProperty("runner_id") String runnerId,
        @JsonProperty("execution_id") String executionId,
        @JsonProperty("status") String status,
        @JsonProperty("error") String error,
        @JsonProperty("duration_ms") Long durationMs,
        @JsonProperty("attempt") int attempt) {

    public static final class Status {
        public static final String SUCCESS = "success";
        public static final String FAILURE = "failure";

        private Status() {}
    }
}
