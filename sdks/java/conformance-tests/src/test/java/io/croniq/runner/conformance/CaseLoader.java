package io.croniq.runner.conformance;

import static io.croniq.runner.conformance.YamlSupport.EXPECTATIONS_KEYS;
import static io.croniq.runner.conformance.YamlSupport.HTTP_EXPECTATION_KEYS;
import static io.croniq.runner.conformance.YamlSupport.loadRoot;
import static io.croniq.runner.conformance.YamlSupport.parseScript;
import static io.croniq.runner.conformance.YamlSupport.requireKnownKeys;

import java.io.IOException;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * Loads a YAML conformance case from disk into a typed {@link CaseSpec}.
 *
 * <p>SnakeYAML produces nested {@code Map<String, Object>} / {@code List<Object>}
 * graphs; the loader picks the few keys it cares about by name and builds the
 * typed record manually. We don't use Jackson YAML here because the case schema
 * is small and snake_case-to-camelCase translation needs case-by-case knowledge
 * (e.g., the {@code body} field stays untyped for {@link BodyMatcher}).
 *
 * <p>Picking keys by name means an unrecognised key is never read at all, so
 * every level asserts its vocabulary through
 * {@link YamlSupport#requireKnownKeys}. Without that, a case using an assertion
 * key this binding has not implemented would load cleanly and simply not be
 * asserted (#460).
 *
 * <p>The generic load and {@code server_script} parsing live in
 * {@link YamlSupport}, shared with {@link TriggerCaseLoader}, so the YAML 1.1
 * {@code on:} workaround and the script vocabulary exist in exactly one place.
 */
final class CaseLoader {

    /**
     * Exactly the keys this binding implements, one set per node a runner case
     * nests. See {@link YamlSupport#requireKnownKeys} for why these are the
     * binding's own surface rather than a copy of case-schema.json.
     */
    private static final Set<String> CASE_KEYS = Set.of(
            "name", "description", "runner_config", "handlers", "server_script", "shutdown_after_ms", "expectations");

    private static final Set<String> RUNNER_CONFIG_KEYS = Set.of(
            "runner_id",
            "runner_id_prefix",
            "capabilities",
            "tags",
            "max_inflight",
            "api_key",
            "bearer_token",
            "poll_timeout_ms",
            "renew_interval_ms",
            "drain_timeout_ms",
            "poll_retry_delay_ms",
            "capacity_backoff_ms",
            "max_consecutive_poll_conflicts",
            "max_consecutive_auth_failures");

    private static final Set<String> HANDLER_KEYS = Set.of(
            "job_key",
            "is_default",
            "schedule",
            "behavior",
            "error_message",
            "duration_ms",
            "level",
            "message",
            "count",
            "interval_ms");

    private CaseLoader() {}

    static CaseSpec load(Path file) throws IOException {
        return parseCase(loadRoot(file));
    }

    private static CaseSpec parseCase(Map<String, Object> m) {
        requireKnownKeys(m, CASE_KEYS, "case");
        return new CaseSpec(
                stringOf(m, "name"),
                stringOf(m, "description"),
                parseRunnerConfig(mapOf(m, "runner_config")),
                parseHandlers(listOf(m, "handlers")),
                parseScript(listOf(m, "server_script")),
                intOf(m, "shutdown_after_ms"),
                parseExpectations(mapOf(m, "expectations")));
    }

    private static CaseSpec.RunnerConfig parseRunnerConfig(Map<String, Object> m) {
        if (m == null) {
            return null;
        }
        requireKnownKeys(m, RUNNER_CONFIG_KEYS, "runner_config");
        return new CaseSpec.RunnerConfig(
                stringOf(m, "runner_id"),
                stringOf(m, "runner_id_prefix"),
                stringListOf(m, "capabilities"),
                stringListOf(m, "tags"),
                intOf(m, "max_inflight"),
                stringOf(m, "api_key"),
                stringOf(m, "bearer_token"),
                intOf(m, "poll_timeout_ms"),
                intOf(m, "renew_interval_ms"),
                intOf(m, "drain_timeout_ms"),
                intOf(m, "poll_retry_delay_ms"),
                intOf(m, "capacity_backoff_ms"),
                intOf(m, "max_consecutive_poll_conflicts"),
                intOf(m, "max_consecutive_auth_failures"));
    }

    private static List<CaseSpec.HandlerSpec> parseHandlers(List<Object> raw) {
        if (raw == null) {
            return List.of();
        }
        List<CaseSpec.HandlerSpec> out = new ArrayList<>(raw.size());
        for (Object o : raw) {
            @SuppressWarnings("unchecked")
            Map<String, Object> h = (Map<String, Object>) o;
            requireKnownKeys(h, HANDLER_KEYS, "handler '%s'".formatted(stringOf(h, "job_key")));
            out.add(new CaseSpec.HandlerSpec(
                    stringOf(h, "job_key"),
                    boolOf(h, "is_default"),
                    stringOf(h, "schedule"),
                    stringOf(h, "behavior"),
                    stringOf(h, "error_message"),
                    intOf(h, "duration_ms"),
                    stringOf(h, "level"),
                    stringOf(h, "message"),
                    intOf(h, "count"),
                    intOf(h, "interval_ms")));
        }
        return out;
    }

    private static CaseSpec.Expectations parseExpectations(Map<String, Object> m) {
        if (m == null) {
            return null;
        }
        requireKnownKeys(m, EXPECTATIONS_KEYS, "expectations");
        List<CaseSpec.Expectations.HttpExpectation> http = new ArrayList<>();
        List<Object> rawHttp = listOf(m, "http");
        if (rawHttp != null) {
            for (Object o : rawHttp) {
                @SuppressWarnings("unchecked")
                Map<String, Object> e = (Map<String, Object>) o;
                requireKnownKeys(e, HTTP_EXPECTATION_KEYS, YamlSupport.httpExpectationContext(e));
                http.add(new CaseSpec.Expectations.HttpExpectation(
                        stringOf(e, "method"),
                        stringOf(e, "path"),
                        intOf(e, "exact_count"),
                        intOf(e, "min_count"),
                        intOf(e, "max_count"),
                        stringMapOf(e, "headers"),
                        e.get("body_match")));
            }
        }
        return new CaseSpec.Expectations(intOf(m, "duration_max_ms"), http);
    }

    // -------- map / type helpers --------
    //
    // stringListOf stays local: it returns an empty list where YamlSupport's
    // returns null, and the runner-config record relies on that. The rest
    // delegate so the coercion rules have a single definition.

    private static List<String> stringListOf(Map<String, Object> m, String key) {
        List<Object> raw = listOf(m, key);
        if (raw == null) {
            return List.of();
        }
        List<String> out = new ArrayList<>(raw.size());
        for (Object o : raw) {
            out.add(o == null ? null : o.toString());
        }
        return out;
    }

    private static Map<String, Object> mapOf(Map<String, Object> m, String key) {
        return YamlSupport.mapOf(m, key);
    }

    private static List<Object> listOf(Map<String, Object> m, String key) {
        return YamlSupport.listOf(m, key);
    }

    private static Map<String, String> stringMapOf(Map<String, Object> m, String key) {
        return YamlSupport.stringMapOf(m, key);
    }

    private static String stringOf(Map<String, Object> m, String key) {
        return YamlSupport.stringOf(m, key);
    }

    private static Integer intOf(Map<String, Object> m, String key) {
        return YamlSupport.intOf(m, key);
    }

    private static Boolean boolOf(Map<String, Object> m, String key) {
        return YamlSupport.boolOf(m, key);
    }
}
