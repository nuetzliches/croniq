package io.croniq.runner.conformance;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import org.yaml.snakeyaml.LoaderOptions;
import org.yaml.snakeyaml.Yaml;
import org.yaml.snakeyaml.constructor.SafeConstructor;

/**
 * Generic SnakeYAML load + type-coercion helpers shared by the case loaders.
 *
 * <p>Both case loaders route through here so the tricky bits exist once — notably
 * {@link #normaliseKeys(Object)}, which works around SnakeYAML 2.x coercing the
 * bare YAML 1.1 keyword {@code on:} (the script-entry key) to a {@link Boolean},
 * and {@link #requireKnownKeys}, which is what stops an unimplemented key from
 * being silently dropped.
 */
final class YamlSupport {

    /**
     * The key vocabulary this binding implements for the nodes both case shapes
     * share. Anything else is a load-time error — see {@link #requireKnownKeys}.
     */
    static final Set<String> SCRIPT_ENTRY_KEYS = Set.of("on", "match_count", "respond");

    static final Set<String> RESPOND_KEYS = Set.of("status", "body", "delay_ms", "headers");

    static final Set<String> EXPECTATIONS_KEYS = Set.of("duration_max_ms", "http");

    static final Set<String> HTTP_EXPECTATION_KEYS =
            Set.of("method", "path", "exact_count", "min_count", "max_count", "headers", "body_match");

    /**
     * Trigger cases additionally pin the omission of unset optionals. Runner
     * cases must not use {@code body_absent} — case-schema.json does not
     * declare it, only trigger-case-schema.json does.
     */
    static final Set<String> TRIGGER_HTTP_EXPECTATION_KEYS = union(HTTP_EXPECTATION_KEYS, "body_absent");

    private YamlSupport() {}

    /** Set union helper — {@code Set.of} has no varargs-append form. */
    static Set<String> union(Set<String> base, String... extra) {
        Set<String> out = new LinkedHashSet<>(base);
        out.addAll(List.of(extra));
        return Set.copyOf(out);
    }

    /**
     * Reject any key {@code allowed} does not list.
     *
     * <p>The loaders pick keys out of the SnakeYAML map by name, so a key they
     * do not ask for is simply never read: a case carrying an assertion this
     * binding has not implemented would load cleanly and then not be asserted —
     * a green suite for an unenforced contract (#460), which is the failure mode
     * of the case-level {@code SCOPE} allowlist (#453) one level down.
     *
     * <p>This is complementary to, not a duplicate of, the corpus-level
     * {@code check-jsonschema} run in CI: that catches a key the <em>schema</em>
     * does not allow, this catches a schema-legal key the <em>binding</em> has
     * not implemented. The sets are therefore expected to lag the schema
     * wherever a capability is .NET-only — {@code runner_config}'s
     * {@code max_consecutive_poll_conflicts} is in the schema but not here,
     * because the Java SDK has no such option, and a case using it must fail
     * loudly rather than run with the option ignored.
     */
    static void requireKnownKeys(Map<String, Object> m, Set<String> allowed, String ctx) {
        if (m == null) {
            return;
        }
        List<String> unknown =
                m.keySet().stream().filter(k -> !allowed.contains(k)).sorted().toList();
        if (!unknown.isEmpty()) {
            throw new IllegalStateException(
                    "%s: unrecognised key(s) %s. This binding does not implement them — either the case is wrong or the Java conformance harness needs updating. Known keys: %s"
                            .formatted(ctx, unknown, allowed.stream().sorted().toList()));
        }
    }

    /** Label an HTTP expectation for a {@link #requireKnownKeys} message. */
    static String httpExpectationContext(Map<String, Object> e) {
        return "http expectation %s %s".formatted(stringOf(e, "method"), stringOf(e, "path"));
    }

    /**
     * Load a YAML document, normalise its keys to strings, and return the top-level map.
     *
     * <p>Constructed with an explicit {@link SafeConstructor}: only standard YAML
     * scalars, sequences and mappings are built, never arbitrary Java types.
     * SnakeYAML 2.x's default {@code TagInspector} already refuses global tags
     * (the CVE-2022-1471 fix) and these fixtures are repo-local, so a bare
     * {@code new Yaml()} is not exploitable today — but that rests on a
     * version-dependent default, and pinning the property here keeps it true if
     * the dependency moves or a fixture ever arrives from outside the repo.
     */
    @SuppressWarnings("unchecked")
    static Map<String, Object> loadRoot(Path file) throws IOException {
        try (InputStream in = Files.newInputStream(file)) {
            Yaml yaml = new Yaml(new SafeConstructor(new LoaderOptions()));
            Object root = normaliseKeys(yaml.load(in));
            if (!(root instanceof Map)) {
                throw new IOException("Top-level YAML must be a map: " + file);
            }
            return (Map<String, Object>) root;
        }
    }

    /**
     * Parse a {@code server_script} list into {@link CaseSpec.ScriptEntry} entries.
     *
     * <p>Shared by both case shapes — the mock-server contract is identical, so
     * the script vocabulary is enforced in exactly one place.
     */
    static List<CaseSpec.ScriptEntry> parseScript(List<Object> raw) {
        if (raw == null) {
            return List.of();
        }
        List<CaseSpec.ScriptEntry> out = new ArrayList<>(raw.size());
        for (Object o : raw) {
            @SuppressWarnings("unchecked")
            Map<String, Object> e = (Map<String, Object>) o;
            requireKnownKeys(e, SCRIPT_ENTRY_KEYS, "server_script entry '%s'".formatted(stringOf(e, "on")));
            @SuppressWarnings("unchecked")
            Map<String, Object> r = (Map<String, Object>) e.get("respond");
            requireKnownKeys(r, RESPOND_KEYS, "respond of '%s'".formatted(stringOf(e, "on")));
            CaseSpec.ScriptEntry.Respond resp = r == null
                    ? null
                    : new CaseSpec.ScriptEntry.Respond(
                            intRequired(r, "status"), r.get("body"), stringMapOf(r, "headers"), intOf(r, "delay_ms"));
            out.add(new CaseSpec.ScriptEntry(stringOf(e, "on"), intOf(e, "match_count"), resp));
        }
        return out;
    }

    @SuppressWarnings("unchecked")
    private static Object normaliseKeys(Object o) {
        if (o instanceof Map<?, ?> m) {
            Map<String, Object> out = new LinkedHashMap<>();
            for (var e : m.entrySet()) {
                out.put(keyToString(e.getKey()), normaliseKeys(e.getValue()));
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

    @SuppressWarnings("unchecked")
    static Map<String, Object> mapOf(Map<String, Object> m, String key) {
        Object v = m.get(key);
        return v instanceof Map ? (Map<String, Object>) v : null;
    }

    @SuppressWarnings("unchecked")
    static List<Object> listOf(Map<String, Object> m, String key) {
        Object v = m.get(key);
        return v instanceof List ? (List<Object>) v : null;
    }

    static List<String> stringListOf(Map<String, Object> m, String key) {
        List<Object> raw = listOf(m, key);
        if (raw == null) {
            return null;
        }
        List<String> out = new ArrayList<>(raw.size());
        for (Object o : raw) {
            out.add(o == null ? null : o.toString());
        }
        return out;
    }

    static Map<String, String> stringMapOf(Map<String, Object> m, String key) {
        Map<String, Object> raw = mapOf(m, key);
        if (raw == null) {
            return Collections.emptyMap();
        }
        Map<String, String> out = new LinkedHashMap<>();
        raw.forEach((k, v) -> out.put(k, v == null ? null : v.toString()));
        return out;
    }

    static String stringOf(Map<String, Object> m, String key) {
        Object v = m.get(key);
        return v == null ? null : v.toString();
    }

    static Integer intOf(Map<String, Object> m, String key) {
        Object v = m.get(key);
        if (v == null) {
            return null;
        }
        if (v instanceof Number n) {
            return n.intValue();
        }
        return Integer.parseInt(v.toString());
    }

    static int intRequired(Map<String, Object> m, String key) {
        Integer v = intOf(m, key);
        if (v == null) {
            throw new IllegalStateException("Missing required int: " + key);
        }
        return v;
    }

    static Boolean boolOf(Map<String, Object> m, String key) {
        Object v = m.get(key);
        return v == null ? null : Boolean.parseBoolean(v.toString());
    }
}
