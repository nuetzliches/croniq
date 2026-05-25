package io.croniq.runner.protocol;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;
import java.util.List;

/** POST /v1/jobs/register request body. Used by self-registration in PR-4. */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record RegisterJobRequest(
        @JsonProperty("job_key") String jobKey,
        @JsonProperty("schedule") String schedule,
        @JsonProperty("timezone") String timezone,
        @JsonProperty("timeout") String timeout,
        @JsonProperty("runner_id") String runnerId,
        @JsonProperty("capabilities") List<String> capabilities,
        @JsonProperty("description") String description) {}
