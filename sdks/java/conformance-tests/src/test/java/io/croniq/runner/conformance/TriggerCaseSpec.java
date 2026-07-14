package io.croniq.runner.conformance;

import java.util.List;
import java.util.Map;

/**
 * In-memory representation of one trigger (producer) conformance case YAML.
 * Mirrors {@code sdks/conformance/schema/trigger-case-schema.json}.
 *
 * <p>A producer case swaps the runner loop for explicit {@link #triggerCalls()}
 * (each a {@code request} the binding sends and an {@code expect} of either a
 * parsed {@code response} or an {@code error}); {@code serverScript} and
 * {@code expectations.http} are shared with the runner cases.
 */
public record TriggerCaseSpec(
        String name,
        String description,
        TriggerConfig triggerConfig,
        List<TriggerCall> triggerCalls,
        List<CaseSpec.ScriptEntry> serverScript,
        Expectations expectations) {

    /** Maps to the trigger client's options ({@code server_url} is injected by the binding). */
    public record TriggerConfig(String apiKey, String bearerToken) {}

    public record TriggerCall(Request request, Expect expect) {

        public record Request(
                String jobKey,
                List<String> require,
                List<String> prefer,
                Map<String, Object> metadata,
                String timeout,
                String idempotencyKey) {}

        /** Exactly one of {@code response} (success) or {@code error} (true) by convention. */
        public record Expect(Response response, Boolean error) {

            public record Response(String executionId, Integer queued, Boolean deduplicated) {}
        }
    }

    public record Expectations(Integer durationMaxMs, List<HttpExpectation> http) {

        public record HttpExpectation(
                String method,
                String path,
                Integer exactCount,
                Integer minCount,
                Integer maxCount,
                Map<String, String> headers,
                Object bodyMatch,
                List<String> bodyAbsent) {}
    }
}
