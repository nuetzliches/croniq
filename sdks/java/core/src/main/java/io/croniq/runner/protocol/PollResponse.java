package io.croniq.runner.protocol;

import com.fasterxml.jackson.annotation.JsonProperty;
import java.util.List;

/** POST /v1/work/poll response body. */
public record PollResponse(
        @JsonProperty("work") List<WorkAssignment> work, @JsonProperty("cancel") List<String> cancel) {

    /** Returns an empty response, used when the server returns {@code null} bodies. */
    public static PollResponse empty() {
        return new PollResponse(List.of(), List.of());
    }
}
