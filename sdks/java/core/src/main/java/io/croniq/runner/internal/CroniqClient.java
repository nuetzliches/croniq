package io.croniq.runner.internal;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import io.croniq.runner.config.CroniqRunnerOptions;
import io.croniq.runner.protocol.AckRequest;
import io.croniq.runner.protocol.PollRequest;
import io.croniq.runner.protocol.PollResponse;
import io.croniq.runner.protocol.RegisterJobRequest;
import io.croniq.runner.protocol.RenewRequest;
import io.croniq.runner.protocol.WorkEvent;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.net.http.HttpResponse.BodyHandlers;
import java.time.Duration;
import java.util.List;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * HTTP transport for the runner. Wraps {@link java.net.http.HttpClient} —
 * keeps the auth header, request bodies, and status-code handling in one place
 * so the dispatcher and poll loop don't need to know about HTTP.
 *
 * <p>Per-request timeouts are scoped to each call rather than the underlying
 * {@code HttpClient}: the poll endpoint uses a long timeout (server-side
 * long-poll), while ack / renew complete in milliseconds. A single
 * {@code HttpClient} instance with no global timeout serves both.
 *
 * <p>Authentication: if {@code apiKey} is set, every request carries
 * {@code Authorization: ApiKey <key>}; otherwise if {@code bearerToken} is set,
 * {@code Authorization: Bearer <token>}. ApiKey takes precedence — matches the
 * .NET SDK and the conformance suite's expectations.
 */
public class CroniqClient {

    private static final Logger log = LoggerFactory.getLogger(CroniqClient.class);
    private static final String CONTENT_TYPE = "application/json";

    private final HttpClient http;
    private final ObjectMapper json;
    private final URI baseUrl;
    private final String authHeader;
    private final String userAgent;

    public CroniqClient(CroniqRunnerOptions options) {
        this(
                HttpClient.newBuilder()
                        // No global timeout: poll calls are long-running and ack/renew are
                        // bounded per-call via HttpRequest.timeout(). Following redirects
                        // is unsafe for an authenticated API — disable.
                        .followRedirects(HttpClient.Redirect.NEVER)
                        .build(),
                options);
    }

    CroniqClient(HttpClient http, CroniqRunnerOptions options) {
        this.http = http;
        this.json = CroniqJsonMapper.instance();
        this.baseUrl = options.serverUrl();
        this.authHeader = buildAuthHeader(options);
        this.userAgent = "croniq-runner-java/" + sdkVersion();
    }

    private static String buildAuthHeader(CroniqRunnerOptions options) {
        if (options.apiKey() != null && !options.apiKey().isBlank()) {
            return "ApiKey " + options.apiKey();
        }
        if (options.bearerToken() != null && !options.bearerToken().isBlank()) {
            return "Bearer " + options.bearerToken();
        }
        return null;
    }

    public PollResponse poll(PollRequest request, Duration timeout) throws IOException, InterruptedException {
        HttpResponse<byte[]> resp = exchange("/v1/work/poll", request, timeout);
        ensureSuccess(resp, "poll");
        if (resp.body() == null || resp.body().length == 0) {
            return PollResponse.empty();
        }
        return json.readValue(resp.body(), PollResponse.class);
    }

    public void ack(AckRequest request) throws IOException, InterruptedException {
        HttpResponse<byte[]> resp = exchange("/v1/work/ack", request, Duration.ofSeconds(15));
        ensureSuccess(resp, "ack");
    }

    public void renew(RenewRequest request) throws IOException, InterruptedException {
        HttpResponse<byte[]> resp = exchange("/v1/work/renew", request, Duration.ofSeconds(15));
        ensureSuccess(resp, "renew");
    }

    public void pushEvents(String executionId, List<WorkEvent> events) throws IOException, InterruptedException {
        if (events.isEmpty()) {
            return;
        }
        HttpResponse<byte[]> resp = exchange("/v1/work/" + executionId + "/events", events, Duration.ofSeconds(15));
        ensureSuccess(resp, "pushEvents");
    }

    public void registerJob(RegisterJobRequest request) throws IOException, InterruptedException {
        HttpResponse<byte[]> resp = exchange("/v1/jobs/register", request, Duration.ofSeconds(15));
        ensureSuccess(resp, "registerJob");
    }

    private HttpResponse<byte[]> exchange(String path, Object body, Duration timeout)
            throws IOException, InterruptedException {
        byte[] payload;
        try {
            payload = json.writeValueAsBytes(body);
        } catch (JsonProcessingException e) {
            throw new IOException("Could not serialise " + body.getClass().getSimpleName(), e);
        }
        HttpRequest.Builder b = HttpRequest.newBuilder()
                .uri(resolve(path))
                .timeout(timeout)
                .header("Content-Type", CONTENT_TYPE)
                .header("Accept", CONTENT_TYPE)
                .header("User-Agent", userAgent)
                .POST(HttpRequest.BodyPublishers.ofByteArray(payload));
        if (authHeader != null) {
            b.header("Authorization", authHeader);
        }
        return http.send(b.build(), BodyHandlers.ofByteArray());
    }

    private URI resolve(String path) {
        String base = baseUrl.toString();
        if (base.endsWith("/")) {
            base = base.substring(0, base.length() - 1);
        }
        return URI.create(base + path);
    }

    private static void ensureSuccess(HttpResponse<byte[]> resp, String op) throws IOException {
        int sc = resp.statusCode();
        if (sc < 200 || sc >= 300) {
            String body = resp.body() == null ? "" : new String(resp.body());
            String snippet = body.length() > 200 ? body.substring(0, 200) + "…" : body;
            log.debug("Croniq {} returned HTTP {}: {}", op, sc, snippet);
            throw new IOException("Croniq " + op + " returned HTTP " + sc + ": " + snippet);
        }
    }

    private static String sdkVersion() {
        Package pkg = CroniqClient.class.getPackage();
        String v = pkg == null ? null : pkg.getImplementationVersion();
        return v != null ? v : "0.0.0-dev";
    }
}
