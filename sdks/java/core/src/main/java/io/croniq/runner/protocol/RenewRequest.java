package io.croniq.runner.protocol;

import com.fasterxml.jackson.annotation.JsonProperty;

/** POST /v1/work/renew request body — lease heartbeat. Used from PR-3 onward. */
public record RenewRequest(
        @JsonProperty("runner_id") String runnerId, @JsonProperty("execution_id") String executionId) {}
