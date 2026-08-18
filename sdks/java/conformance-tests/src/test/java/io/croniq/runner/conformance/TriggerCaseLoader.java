package io.croniq.runner.conformance;

import static io.croniq.runner.conformance.YamlSupport.EXPECTATIONS_KEYS;
import static io.croniq.runner.conformance.YamlSupport.TRIGGER_HTTP_EXPECTATION_KEYS;
import static io.croniq.runner.conformance.YamlSupport.boolOf;
import static io.croniq.runner.conformance.YamlSupport.httpExpectationContext;
import static io.croniq.runner.conformance.YamlSupport.intOf;
import static io.croniq.runner.conformance.YamlSupport.listOf;
import static io.croniq.runner.conformance.YamlSupport.loadRoot;
import static io.croniq.runner.conformance.YamlSupport.mapOf;
import static io.croniq.runner.conformance.YamlSupport.parseScript;
import static io.croniq.runner.conformance.YamlSupport.requireKnownKeys;
import static io.croniq.runner.conformance.YamlSupport.stringListOf;
import static io.croniq.runner.conformance.YamlSupport.stringMapOf;
import static io.croniq.runner.conformance.YamlSupport.stringOf;

import java.io.IOException;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Set;

/**
 * Loads a trigger (producer) conformance case YAML into a {@link TriggerCaseSpec}.
 *
 * <p>Every level asserts its key vocabulary through
 * {@link YamlSupport#requireKnownKeys}: keys are picked out of the SnakeYAML map
 * by name, so one this loader does not ask for would otherwise be dropped in
 * silence (#460).
 */
final class TriggerCaseLoader {

    /** Exactly the keys this binding implements, per node a trigger case nests. */
    private static final Set<String> TRIGGER_CASE_KEYS =
            Set.of("name", "description", "trigger_config", "trigger_calls", "server_script", "expectations");

    private static final Set<String> TRIGGER_CONFIG_KEYS = Set.of("api_key", "bearer_token");

    private static final Set<String> TRIGGER_CALL_KEYS = Set.of("request", "expect");

    private static final Set<String> TRIGGER_REQUEST_KEYS =
            Set.of("job_key", "require", "prefer", "metadata", "timeout", "idempotency_key");

    private static final Set<String> TRIGGER_EXPECT_KEYS = Set.of("response", "error");

    /**
     * Asserted one field at a time by {@link TriggerConformanceRunner}, so an
     * unrecognised key here is the silent-drop case exactly and has to be
     * rejected up front.
     */
    private static final Set<String> TRIGGER_RESPONSE_KEYS = Set.of("execution_id", "queued", "deduplicated");

    private TriggerCaseLoader() {}

    static TriggerCaseSpec load(Path file) throws IOException {
        Map<String, Object> m = loadRoot(file);
        requireKnownKeys(m, TRIGGER_CASE_KEYS, "trigger case");
        return new TriggerCaseSpec(
                stringOf(m, "name"),
                stringOf(m, "description"),
                parseTriggerConfig(mapOf(m, "trigger_config")),
                parseTriggerCalls(listOf(m, "trigger_calls")),
                parseScript(listOf(m, "server_script")),
                parseExpectations(mapOf(m, "expectations")));
    }

    private static TriggerCaseSpec.TriggerConfig parseTriggerConfig(Map<String, Object> m) {
        if (m == null) {
            return null;
        }
        requireKnownKeys(m, TRIGGER_CONFIG_KEYS, "trigger_config");
        return new TriggerCaseSpec.TriggerConfig(stringOf(m, "api_key"), stringOf(m, "bearer_token"));
    }

    private static List<TriggerCaseSpec.TriggerCall> parseTriggerCalls(List<Object> raw) {
        if (raw == null) {
            return List.of();
        }
        List<TriggerCaseSpec.TriggerCall> out = new ArrayList<>(raw.size());
        for (Object o : raw) {
            @SuppressWarnings("unchecked")
            Map<String, Object> call = (Map<String, Object>) o;
            requireKnownKeys(call, TRIGGER_CALL_KEYS, "trigger_calls entry");
            out.add(new TriggerCaseSpec.TriggerCall(
                    parseRequest(mapOf(call, "request")), parseExpect(mapOf(call, "expect"))));
        }
        return out;
    }

    private static TriggerCaseSpec.TriggerCall.Request parseRequest(Map<String, Object> m) {
        if (m == null) {
            return null;
        }
        requireKnownKeys(m, TRIGGER_REQUEST_KEYS, requestContext(m));
        return new TriggerCaseSpec.TriggerCall.Request(
                stringOf(m, "job_key"),
                stringListOf(m, "require"),
                stringListOf(m, "prefer"),
                mapOf(m, "metadata"),
                stringOf(m, "timeout"),
                stringOf(m, "idempotency_key"));
    }

    private static TriggerCaseSpec.TriggerCall.Expect parseExpect(Map<String, Object> m) {
        if (m == null) {
            return null;
        }
        requireKnownKeys(m, TRIGGER_EXPECT_KEYS, "trigger_calls expect");
        Map<String, Object> resp = mapOf(m, "response");
        requireKnownKeys(resp, TRIGGER_RESPONSE_KEYS, "trigger_calls expect.response");
        TriggerCaseSpec.TriggerCall.Expect.Response response = resp == null
                ? null
                : new TriggerCaseSpec.TriggerCall.Expect.Response(
                        stringOf(resp, "execution_id"), intOf(resp, "queued"), boolOf(resp, "deduplicated"));
        return new TriggerCaseSpec.TriggerCall.Expect(response, boolOf(m, "error"));
    }

    private static TriggerCaseSpec.Expectations parseExpectations(Map<String, Object> m) {
        if (m == null) {
            return null;
        }
        requireKnownKeys(m, EXPECTATIONS_KEYS, "expectations");
        List<TriggerCaseSpec.Expectations.HttpExpectation> http = new ArrayList<>();
        List<Object> rawHttp = listOf(m, "http");
        if (rawHttp != null) {
            for (Object o : rawHttp) {
                @SuppressWarnings("unchecked")
                Map<String, Object> e = (Map<String, Object>) o;
                requireKnownKeys(e, TRIGGER_HTTP_EXPECTATION_KEYS, httpExpectationContext(e));
                http.add(new TriggerCaseSpec.Expectations.HttpExpectation(
                        stringOf(e, "method"),
                        stringOf(e, "path"),
                        intOf(e, "exact_count"),
                        intOf(e, "min_count"),
                        intOf(e, "max_count"),
                        stringMapOf(e, "headers"),
                        e.get("body_match"),
                        stringListOf(e, "body_absent")));
            }
        }
        return new TriggerCaseSpec.Expectations(intOf(m, "duration_max_ms"), http);
    }

    private static String requestContext(Map<String, Object> m) {
        return "trigger_calls request '%s'".formatted(stringOf(m, "job_key"));
    }
}
