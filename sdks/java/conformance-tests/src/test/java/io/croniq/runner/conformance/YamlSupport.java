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
import org.yaml.snakeyaml.LoaderOptions;
import org.yaml.snakeyaml.Yaml;
import org.yaml.snakeyaml.constructor.SafeConstructor;

/**
 * Generic SnakeYAML load + type-coercion helpers shared by the case loaders.
 *
 * <p>Kept separate from {@link CaseLoader} (which has its own private copies for
 * the runner cases) so the trigger loader can reuse the tricky bits — notably
 * {@link #normaliseKeys(Object)}, which works around SnakeYAML 2.x coercing the
 * bare YAML 1.1 keyword {@code on:} (the script-entry key) to a {@link Boolean}.
 */
final class YamlSupport {

    private YamlSupport() {}

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

    /** Parse a {@code server_script} list into {@link CaseSpec.ScriptEntry} entries. */
    static List<CaseSpec.ScriptEntry> parseScript(List<Object> raw) {
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
