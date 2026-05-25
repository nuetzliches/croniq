package io.croniq.runner.conformance;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.yaml.snakeyaml.Yaml;

/**
 * Loads a YAML conformance case from disk into a typed {@link CaseSpec}.
 *
 * <p>SnakeYAML produces nested {@code Map<String, Object>} / {@code List<Object>}
 * graphs; the loader picks the few keys it cares about by name and builds the
 * typed record manually. We don't use Jackson YAML here because the case schema
 * is small and snake_case-to-camelCase translation needs case-by-case knowledge
 * (e.g., the {@code body} field stays untyped for {@link BodyMatcher}).
 */
final class CaseLoader {

    private CaseLoader() {}

    static CaseSpec load(Path file) throws IOException {
        try (InputStream in = Files.newInputStream(file)) {
            Yaml yaml = new Yaml();
            Object root = yaml.load(in);
            // SnakeYAML 2.x still treats bare YAML 1.1 keywords (`on`, `off`,
            // `yes`, `no`) as booleans even though it nominally defaults to
            // YAML 1.2. The case files use `on:` as the script-entry key — we
            // need to coerce every map key back to its string form before any
            // typed access, otherwise `parseCase` looks up "on" and misses.
            root = normaliseKeys(root);
            if (!(root instanceof Map)) {
                throw new IOException("Top-level YAML must be a map: " + file);
            }
            @SuppressWarnings("unchecked")
            Map<String, Object> top = (Map<String, Object>) root;
            return parseCase(top);
        }
    }

    @SuppressWarnings("unchecked")
    private static Object normaliseKeys(Object o) {
        if (o instanceof Map<?, ?> m) {
            Map<String, Object> out = new LinkedHashMap<>();
            for (var e : m.entrySet()) {
                String key = keyToString(e.getKey());
                out.put(key, normaliseKeys(e.getValue()));
            }
            return out;
        }
        if (o instanceof List<?> list) {
            List<Object> out = new ArrayList<>(list.size());
            for (Object item : list) {
                out.add(normaliseKeys(item));
            }
            return out;
        }
        return o;
    }

    /**
     * SnakeYAML 2.x maps YAML 1.1 keywords {@code on}, {@code off}, {@code yes},
     * {@code no} to {@link Boolean}. We can't recover the exact original
     * spelling but the case schema only uses {@code on:} as a key, so
     * {@code Boolean.TRUE} → "on" and {@code Boolean.FALSE} → "off" is a safe
     * round-trip for our YAML files. Numeric keys would also be unusual at the
     * key position, so cast everything via {@link Object#toString()} as a
     * fallback.
     */
    private static String keyToString(Object key) {
        if (key == null) {
            return null;
        }
        if (key == Boolean.TRUE) {
            return "on";
        }
        if (key == Boolean.FALSE) {
            return "off";
        }
        return key.toString();
    }

    private static CaseSpec parseCase(Map<String, Object> m) {
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
                intOf(m, "capacity_backoff_ms"));
    }

    private static List<CaseSpec.HandlerSpec> parseHandlers(List<Object> raw) {
        if (raw == null) {
            return List.of();
        }
        List<CaseSpec.HandlerSpec> out = new ArrayList<>(raw.size());
        for (Object o : raw) {
            @SuppressWarnings("unchecked")
            Map<String, Object> h = (Map<String, Object>) o;
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

    private static List<CaseSpec.ScriptEntry> parseScript(List<Object> raw) {
        if (raw == null) {
            return List.of();
        }
        List<CaseSpec.ScriptEntry> out = new ArrayList<>(raw.size());
        for (Object o : raw) {
            @SuppressWarnings("unchecked")
            Map<String, Object> e = (Map<String, Object>) o;
            @SuppressWarnings("unchecked")
            Map<String, Object> r = (Map<String, Object>) e.get("respond");
            CaseSpec.ScriptEntry.Respond resp = r == null
                    ? null
                    : new CaseSpec.ScriptEntry.Respond(
                            intRequired(r, "status"), r.get("body"), stringMapOf(r, "headers"), intOf(r, "delay_ms"));
            out.add(new CaseSpec.ScriptEntry(stringOf(e, "on"), intOf(e, "match_count"), resp));
        }
        return out;
    }

    private static CaseSpec.Expectations parseExpectations(Map<String, Object> m) {
        if (m == null) {
            return null;
        }
        List<CaseSpec.Expectations.HttpExpectation> http = new ArrayList<>();
        List<Object> rawHttp = listOf(m, "http");
        if (rawHttp != null) {
            for (Object o : rawHttp) {
                @SuppressWarnings("unchecked")
                Map<String, Object> e = (Map<String, Object>) o;
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

    @SuppressWarnings("unchecked")
    private static Map<String, Object> mapOf(Map<String, Object> m, String key) {
        Object v = m.get(key);
        return v instanceof Map ? (Map<String, Object>) v : null;
    }

    @SuppressWarnings("unchecked")
    private static List<Object> listOf(Map<String, Object> m, String key) {
        Object v = m.get(key);
        return v instanceof List ? (List<Object>) v : null;
    }

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

    private static Map<String, String> stringMapOf(Map<String, Object> m, String key) {
        Map<String, Object> raw = mapOf(m, key);
        if (raw == null) {
            return Collections.emptyMap();
        }
        Map<String, String> out = new LinkedHashMap<>();
        raw.forEach((k, v) -> out.put(k, v == null ? null : v.toString()));
        return out;
    }

    private static String stringOf(Map<String, Object> m, String key) {
        Object v = m.get(key);
        return v == null ? null : v.toString();
    }

    private static Integer intOf(Map<String, Object> m, String key) {
        Object v = m.get(key);
        if (v == null) {
            return null;
        }
        if (v instanceof Integer i) {
            return i;
        }
        if (v instanceof Long l) {
            return Math.toIntExact(l);
        }
        if (v instanceof Number n) {
            return n.intValue();
        }
        return Integer.parseInt(v.toString());
    }

    private static int intRequired(Map<String, Object> m, String key) {
        Integer v = intOf(m, key);
        if (v == null) {
            throw new IllegalStateException("Missing required int: " + key);
        }
        return v;
    }

    private static Boolean boolOf(Map<String, Object> m, String key) {
        Object v = m.get(key);
        if (v == null) {
            return null;
        }
        return Boolean.parseBoolean(v.toString());
    }
}
