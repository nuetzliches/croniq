package io.croniq.runner.conformance;

import java.util.List;
import java.util.Map;

/**
 * In-memory representation of one conformance case YAML. Mirrors the
 * structure declared in {@code sdks/conformance/schema/case-schema.json}.
 *
 * <p>Field names use camelCase; the YAML keys are snake_case and translated
 * by {@link CaseLoader}. Nested {@code body} payloads and {@code body_match}
 * specs keep their loose Map/List form so {@link BodyMatcher} can do JSON
 * subset matching without an intermediate object model.
 */
public record CaseSpec(
        String name,
        String description,
        RunnerConfig runnerConfig,
        List<HandlerSpec> handlers,
        List<ScriptEntry> serverScript,
        Integer shutdownAfterMs,
        Expectations expectations) {

    public record RunnerConfig(
            String runnerId,
            String runnerIdPrefix,
            List<String> capabilities,
            List<String> tags,
            Integer maxInflight,
            String apiKey,
            String bearerToken,
            Integer pollTimeoutMs,
            Integer renewIntervalMs,
            Integer drainTimeoutMs,
            Integer pollRetryDelayMs,
            Integer capacityBackoffMs,
            Integer maxConsecutivePollConflicts,
            Integer maxConsecutiveAuthFailures) {}

    public record HandlerSpec(
            String jobKey,
            Boolean isDefault,
            String schedule,
            String behavior,
            String errorMessage,
            Integer durationMs,
            String level,
            String message,
            Integer count,
            Integer intervalMs) {}

    public record ScriptEntry(String on, Integer matchCount, Respond respond) {

        public record Respond(int status, Object body, Map<String, String> headers, Integer delayMs) {}
    }

    public record Expectations(Integer durationMaxMs, List<HttpExpectation> http) {

        public record HttpExpectation(
                String method,
                String path,
                Integer exactCount,
                Integer minCount,
                Integer maxCount,
                Map<String, String> headers,
                Object bodyMatch) {}
    }
}
