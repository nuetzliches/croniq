package io.croniq.runner.protocol;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.annotation.JsonProperty;
import java.util.List;

/** POST /v1/work/poll request body. */
@JsonInclude(JsonInclude.Include.NON_NULL)
public record PollRequest(
        @JsonProperty("runner_id") String runnerId,
        @JsonProperty("capabilities") List<String> capabilities,
        @JsonProperty("max_inflight") int maxInflight,
        @JsonProperty("inflight") List<String> inflight,
        @JsonProperty("instance_id") String instanceId,
        @JsonProperty("tags") List<String> tags) {}
