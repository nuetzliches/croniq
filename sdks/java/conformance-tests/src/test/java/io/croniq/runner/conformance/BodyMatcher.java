package io.croniq.runner.conformance;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import java.util.List;
import java.util.Map;

/**
 * Subset matcher with one wildcard token. Used to assert that a recorded
 * request body (parsed as a {@link JsonNode}) satisfies the YAML
 * {@code body_match} spec.
 *
 * <p>Rules:
 *
 * <ul>
 *   <li>YAML {@code null} → actual must be JSON {@code null}.
 *   <li>YAML {@code "*"} → actual must not be null or empty.
 *   <li>Map (YAML object) → every listed key must exist with a matching value;
 *       extra keys in the actual body are ignored.
 *   <li>List (YAML array) → lengths must match; each element must match.
 *   <li>Scalar (string / number / boolean) → exact equality.
 * </ul>
 *
 * <p>Returns {@code null} on success; otherwise a path-rooted error like
 * {@code "$.runner_id: expected 'exec-001' but got 'exec-002'"}.
 */
final class BodyMatcher {

    private BodyMatcher() {}

    static String match(Object expected, JsonNode actual) {
        return matchAt("$", expected, actual);
    }

    private static String matchAt(String path, Object expected, JsonNode actual) {
        if (expected == null) {
            if (actual == null || actual.isNull()) {
                return null;
            }
            return path + ": expected null but got " + actual.asText();
        }
        if (expected instanceof String s) {
            if ("*".equals(s)) {
                if (actual == null || actual.isNull()) {
                    return path + ": expected non-empty (*) but was null";
                }
                if (actual.isTextual() && actual.asText().isEmpty()) {
                    return path + ": expected non-empty (*) but was empty string";
                }
                return null;
            }
            if (actual == null || !actual.isTextual()) {
                return path + ": expected string '" + s + "' but got " + describe(actual);
            }
            return s.equals(actual.asText()) ? null : path + ": expected '" + s + "' but got '" + actual.asText() + "'";
        }
        if (expected instanceof Boolean b) {
            if (actual == null || !actual.isBoolean()) {
                return path + ": expected bool " + b + " but got " + describe(actual);
            }
            return b == actual.asBoolean() ? null : path + ": expected " + b + " but got " + actual.asBoolean();
        }
        if (expected instanceof Number n) {
            if (actual == null || !actual.isNumber()) {
                return path + ": expected number " + n + " but got " + describe(actual);
            }
            // Compare as long when possible, else double — the YAML loader
            // doesn't preserve the original literal type so widen with care.
            if (n.longValue() == actual.asLong() && n.doubleValue() == actual.asDouble()) {
                return null;
            }
            return path + ": expected " + n + " but got " + actual.asText();
        }
        if (expected instanceof Map<?, ?> m) {
            if (actual == null || !(actual instanceof ObjectNode)) {
                return path + ": expected object but got " + describe(actual);
            }
            for (var e : m.entrySet()) {
                String key = e.getKey().toString();
                JsonNode child = actual.get(key);
                String err = matchAt(path + "." + key, e.getValue(), child);
                if (err != null) {
                    return err;
                }
            }
            return null;
        }
        if (expected instanceof List<?> list) {
            if (actual == null || !(actual instanceof ArrayNode arr)) {
                return path + ": expected array but got " + describe(actual);
            }
            if (arr.size() != list.size()) {
                return path + ": expected length " + list.size() + " but got " + arr.size();
            }
            for (int i = 0; i < list.size(); i++) {
                String err = matchAt(path + "[" + i + "]", list.get(i), arr.get(i));
                if (err != null) {
                    return err;
                }
            }
            return null;
        }
        return path + ": unsupported expected type " + expected.getClass().getName();
    }

    private static String describe(JsonNode n) {
        if (n == null) {
            return "missing";
        }
        if (n.isNull()) {
            return "null";
        }
        return n.getNodeType() + " '" + n.asText() + "'";
    }
}
