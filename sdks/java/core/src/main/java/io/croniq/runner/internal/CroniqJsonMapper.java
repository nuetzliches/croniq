package io.croniq.runner.internal;

import com.fasterxml.jackson.databind.DeserializationFeature;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.json.JsonMapper;
import com.fasterxml.jackson.datatype.jsr310.JavaTimeModule;

/**
 * Single shared {@link ObjectMapper} used across the SDK. Holds the wire-protocol
 * conventions in one place so we never accidentally diverge between client,
 * conformance harness, and downstream modules.
 *
 * <p>Conventions:
 *
 * <ul>
 *   <li>snake_case JSON keys are enforced at the field level via
 *       {@code @JsonProperty} annotations on each DTO record component. We
 *       don't rely on a global naming strategy because that would silently
 *       affect any third-party {@link com.fasterxml.jackson.databind.JsonNode}
 *       traversal users do against handler metadata.
 *   <li>{@code java.time} types serialise/parse as ISO-8601 strings via the
 *       {@link JavaTimeModule}.
 *   <li>Unknown fields are ignored on the way in — forward-compatible with
 *       server versions that add new keys.
 * </ul>
 */
final class CroniqJsonMapper {

    private static final ObjectMapper INSTANCE = JsonMapper.builder()
            .addModule(new JavaTimeModule())
            .configure(DeserializationFeature.FAIL_ON_UNKNOWN_PROPERTIES, false)
            .build();

    private CroniqJsonMapper() {}

    static ObjectMapper instance() {
        return INSTANCE;
    }
}
