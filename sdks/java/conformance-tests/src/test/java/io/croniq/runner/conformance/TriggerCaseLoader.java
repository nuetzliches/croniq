package io.croniq.runner.conformance;

import static io.croniq.runner.conformance.YamlSupport.boolOf;
import static io.croniq.runner.conformance.YamlSupport.intOf;
import static io.croniq.runner.conformance.YamlSupport.listOf;
import static io.croniq.runner.conformance.YamlSupport.loadRoot;
import static io.croniq.runner.conformance.YamlSupport.mapOf;
import static io.croniq.runner.conformance.YamlSupport.parseScript;
import static io.croniq.runner.conformance.YamlSupport.stringListOf;
import static io.croniq.runner.conformance.YamlSupport.stringMapOf;
import static io.croniq.runner.conformance.YamlSupport.stringOf;

import java.io.IOException;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/** Loads a trigger (producer) conformance case YAML into a {@link TriggerCaseSpec}. */
final class TriggerCaseLoader {

    private TriggerCaseLoader() {}

    static TriggerCaseSpec load(Path file) throws IOException {
        Map<String, Object> m = loadRoot(file);
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
            out.add(new TriggerCaseSpec.TriggerCall(
                    parseRequest(mapOf(call, "request")), parseExpect(mapOf(call, "expect"))));
        }
        return out;
    }

    private static TriggerCaseSpec.TriggerCall.Request parseRequest(Map<String, Object> m) {
        if (m == null) {
            return null;
        }
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
        Map<String, Object> resp = mapOf(m, "response");
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
        List<TriggerCaseSpec.Expectations.HttpExpectation> http = new ArrayList<>();
        List<Object> rawHttp = listOf(m, "http");
        if (rawHttp != null) {
            for (Object o : rawHttp) {
                @SuppressWarnings("unchecked")
                Map<String, Object> e = (Map<String, Object>) o;
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
}
