package io.croniq.runner.conformance;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Tiny HTTP server that replays a case's {@code server_script}. Sequential
 * matching: routes are keyed by {@code "<METHOD> <path>"}, each route has an
 * atomic hit counter, and on every request the first matching script entry
 * (by {@code match_count == counter+1}, or fallthrough if no numbered rule
 * matches) wins. Mirrors the .NET binding's SequentialResponseProvider.
 *
 * <p>We use {@link com.sun.net.httpserver.HttpServer} (in the {@code
 * jdk.httpserver} module — officially supported, not a {@code sun.*} type) so
 * the harness has no extra runtime dependencies beyond the JDK and Jackson
 * (which the SDK already pulls).
 */
final class MockServerHarness implements AutoCloseable {

    private static final Pattern ON_PATTERN = Pattern.compile("^(GET|POST|PUT|DELETE|PATCH)\\s+(/\\S*)$");
    private static final ObjectMapper JSON = new ObjectMapper();

    private final HttpServer server;
    private final Map<String, List<CaseSpec.ScriptEntry>> byRoute = new LinkedHashMap<>();
    private final Map<String, AtomicInteger> counters = new ConcurrentHashMap<>();
    private final List<RecordedRequest> recorded = Collections.synchronizedList(new ArrayList<>());

    MockServerHarness(List<CaseSpec.ScriptEntry> script) throws IOException {
        this.server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        // Bounded executor — the runner sends one request at a time per loop
        // iteration, but cases with parallel handlers (max_inflight > 1) can
        // double up on /v1/work/ack. 8 threads is comfortable headroom.
        server.setExecutor(Executors.newFixedThreadPool(8, r -> {
            Thread t = new Thread(r, "mock-http");
            t.setDaemon(true);
            return t;
        }));
        for (CaseSpec.ScriptEntry e : script) {
            byRoute.computeIfAbsent(e.on(), k -> new ArrayList<>()).add(e);
        }
        server.createContext("/", this::dispatch);
    }

    void start() {
        server.start();
    }

    String baseUrl() {
        return "http://" + server.getAddress().getHostString() + ":"
                + server.getAddress().getPort();
    }

    List<RecordedRequest> recorded() {
        synchronized (recorded) {
            return new ArrayList<>(recorded);
        }
    }

    @Override
    public void close() {
        server.stop(0);
    }

    private void dispatch(HttpExchange ex) throws IOException {
        try {
            dispatchUnsafe(ex);
        } catch (RuntimeException e) {
            // Any unhandled error in the handler causes the HttpServer to
            // close the connection without sending a response — the client
            // then sees "HTTP/1.1 header parser received no bytes" with zero
            // diagnostic info. Log the real cause and synthesise a 500.
            System.err.println("[MockServerHarness] dispatch failed: " + e);
            e.printStackTrace(System.err);
            try {
                send(ex, 500, ("{\"error\":\"" + e.getMessage() + "\"}").getBytes(StandardCharsets.UTF_8), null);
            } catch (IOException sendError) {
                // best-effort
            }
        }
    }

    private void dispatchUnsafe(HttpExchange ex) throws IOException {
        String method = ex.getRequestMethod();
        String path = ex.getRequestURI().getPath();
        Map<String, String> headers = headersOf(ex);
        String body;
        try (InputStream in = ex.getRequestBody()) {
            body = new String(in.readAllBytes(), StandardCharsets.UTF_8);
        }
        // Record FIRST so the test can see the request even if we 404 below.
        recorded.add(new RecordedRequest(method, path, headers, body));

        CaseSpec.ScriptEntry match = chooseEntry(method, path);
        if (match == null) {
            send(ex, 404, "{\"error\":\"no scripted entry for " + method + " " + path + "\"}", null);
            return;
        }
        var r = match.respond();
        if (r.delayMs() != null && r.delayMs() > 0) {
            try {
                Thread.sleep(r.delayMs());
            } catch (InterruptedException ie) {
                Thread.currentThread().interrupt();
            }
        }
        byte[] payload = serialise(r.body());
        Map<String, String> respHeaders = new LinkedHashMap<>();
        if (r.headers() != null) {
            respHeaders.putAll(r.headers());
        }
        if (payload.length > 0 && !respHeaders.containsKey("Content-Type")) {
            respHeaders.put("Content-Type", "application/json");
        }
        send(ex, r.status(), payload, respHeaders);
    }

    private CaseSpec.ScriptEntry chooseEntry(String method, String path) {
        // Find the first route whose key matches this method+path. Routes are
        // declared by the case as exact patterns like "POST /v1/work/poll" —
        // path with placeholders (e.g., /v1/work/{id}/events) is supported by
        // converting the placeholder segment to a wildcard.
        for (var entry : byRoute.entrySet()) {
            Matcher m = ON_PATTERN.matcher(entry.getKey());
            if (!m.matches()) {
                continue;
            }
            if (!method.equalsIgnoreCase(m.group(1))) {
                continue;
            }
            if (!pathMatches(m.group(2), path)) {
                continue;
            }
            // Picked a route — advance its counter and pick the right entry.
            AtomicInteger counter = counters.computeIfAbsent(entry.getKey(), k -> new AtomicInteger(0));
            int n = counter.incrementAndGet();
            for (CaseSpec.ScriptEntry script : entry.getValue()) {
                if (script.matchCount() != null && script.matchCount() == n) {
                    return script;
                }
            }
            // No numbered match — pick the first fallthrough (no match_count).
            for (CaseSpec.ScriptEntry script : entry.getValue()) {
                if (script.matchCount() == null) {
                    return script;
                }
            }
            return null;
        }
        return null;
    }

    /** Matches a case's pattern like {@code /v1/work/{id}/events} against an actual path. */
    private static boolean pathMatches(String pattern, String actual) {
        if (pattern.equals(actual)) {
            return true;
        }
        if (!pattern.contains("{")) {
            return false;
        }
        // Convert {param} to a non-slash segment regex.
        String regex = pattern.replaceAll("\\{[^/}]+\\}", "[^/]+");
        return actual.matches(regex);
    }

    private static Map<String, String> headersOf(HttpExchange ex) {
        Map<String, String> out = new LinkedHashMap<>();
        ex.getRequestHeaders().forEach((name, values) -> {
            if (!values.isEmpty()) {
                // Case-insensitive lookups in expectations — lowercase keys for
                // a stable canonical form.
                out.put(name.toLowerCase(), values.get(0));
            }
        });
        return out;
    }

    private static byte[] serialise(Object body) {
        if (body == null) {
            return new byte[0];
        }
        if (body instanceof byte[] b) {
            return b;
        }
        if (body instanceof String s) {
            return s.getBytes(StandardCharsets.UTF_8);
        }
        try {
            return JSON.writeValueAsBytes(body);
        } catch (Exception e) {
            throw new IllegalStateException("Cannot serialise scripted body", e);
        }
    }

    private static void send(HttpExchange ex, int status, byte[] body, Map<String, String> headers) throws IOException {
        if (headers != null) {
            for (var h : headers.entrySet()) {
                ex.getResponseHeaders().add(h.getKey(), h.getValue());
            }
        }
        ex.sendResponseHeaders(status, body.length == 0 ? -1 : body.length);
        if (body.length > 0) {
            try (OutputStream out = ex.getResponseBody()) {
                out.write(body);
            }
        }
    }

    private static void send(HttpExchange ex, int status, String body, Map<String, String> headers) throws IOException {
        send(ex, status, body == null ? new byte[0] : body.getBytes(StandardCharsets.UTF_8), headers);
    }

    /** A recorded request — what the SDK sent. Used by the assertion phase. */
    record RecordedRequest(String method, String path, Map<String, String> headers, String body) {

        boolean matches(String wantMethod, String wantPath) {
            return wantMethod.equalsIgnoreCase(method) && path.equals(wantPath);
        }
    }
}
