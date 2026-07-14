package io.croniq.runner;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import io.croniq.runner.config.CroniqClientOptions;
import io.croniq.runner.internal.CroniqJsonMapper;
import io.croniq.runner.protocol.TriggerResponse;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.net.http.HttpResponse.BodyHandlers;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.Objects;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Producer-side client for firing Croniq jobs on demand via
 * {@code POST /v1/trigger}. Independent of {@link CroniqRunner} — a pure
 * producer does not need a runner, and the client uses its own credentials
 * ({@link CroniqClientOptions}): the endpoint requires the {@code jobs:trigger}
 * (or {@code admin}) scope, which runner poll keys typically do not carry.
 *
 * <p>Mirrors the .NET SDK's {@code ICroniqTriggerClient}. Requests carry
 * snake_case JSON through the SDK's shared {@link ObjectMapper}; unset optional
 * fields are omitted from the body. Non-2xx responses, transport failures, and
 * serialisation errors surface as {@link CroniqTriggerException} (never a
 * default/empty result).
 *
 * <p>Thread-safe and intended to be long-lived: build one per server + credential
 * and share it. The per-request timeout comes from
 * {@link CroniqClientOptions#requestTimeout()} and is applied per call, so the
 * underlying {@link HttpClient} carries no global timeout.
 */
public final class CroniqTriggerClient {

    private static final Logger log = LoggerFactory.getLogger(CroniqTriggerClient.class);
    private static final String CONTENT_TYPE = "application/json";
    private static final int BODY_SNIPPET_MAX = 200;

    private final HttpClient http;
    private final ObjectMapper json;
    private final URI baseUrl;
    private final String authHeader;
    private final Duration requestTimeout;
    private final String userAgent;

    public CroniqTriggerClient(CroniqClientOptions options) {
        this(
                options,
                // Following redirects is unsafe for an authenticated API — disable.
                HttpClient.newBuilder()
                        .followRedirects(HttpClient.Redirect.NEVER)
                        .build());
    }

    CroniqTriggerClient(CroniqClientOptions options, HttpClient http) {
        Objects.requireNonNull(options, "options");
        this.http = Objects.requireNonNull(http, "http");
        this.json = CroniqJsonMapper.instance();
        this.baseUrl = options.serverUrl();
        this.authHeader = buildAuthHeader(options);
        this.requestTimeout = options.requestTimeout();
        this.userAgent = "croniq-trigger-java/" + sdkVersion();
    }

    /** Fire {@code jobKey} with no metadata, routing hints, timeout, or idempotency key. */
    public TriggerResult trigger(String jobKey) {
        return trigger(TriggerRequest.builder(jobKey).build());
    }

    /**
     * Fire a job immediately. The job's registered handler runs on the next
     * eligible runner, exactly like a scheduled fire.
     *
     * @param request the trigger to send; build with {@link TriggerRequest#builder(String)}.
     * @return the created (or deduplicated) execution and queue depth.
     * @throws CroniqTriggerException on a non-2xx response, transport failure,
     *     or serialisation error. {@link CroniqTriggerException#isQueueOverflow()}
     *     distinguishes the {@code 429} per-job queue-overflow backpressure.
     * @throws IllegalArgumentException if the job key is blank.
     */
    public TriggerResult trigger(TriggerRequest request) {
        Objects.requireNonNull(request, "request");
        if (request.jobKey().isBlank()) {
            throw new IllegalArgumentException("jobKey must not be blank");
        }

        byte[] payload;
        try {
            payload = json.writeValueAsBytes(request);
        } catch (JsonProcessingException e) {
            throw new CroniqTriggerException("Could not serialise trigger request", e);
        }

        HttpRequest.Builder b = HttpRequest.newBuilder()
                .uri(resolve("/v1/trigger"))
                .timeout(requestTimeout)
                .header("Content-Type", CONTENT_TYPE)
                .header("Accept", CONTENT_TYPE)
                .header("User-Agent", userAgent)
                .POST(HttpRequest.BodyPublishers.ofByteArray(payload));
        if (authHeader != null) {
            b.header("Authorization", authHeader);
        }

        HttpResponse<byte[]> resp;
        try {
            resp = http.send(b.build(), BodyHandlers.ofByteArray());
        } catch (IOException e) {
            throw new CroniqTriggerException("POST /v1/trigger failed: " + e.getMessage(), e);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new CroniqTriggerException("POST /v1/trigger was interrupted", e);
        }

        int status = resp.statusCode();
        if (status < 200 || status >= 300) {
            String snippet = snippet(resp.body());
            log.debug("Croniq trigger returned HTTP {}: {}", status, snippet);
            throw new CroniqTriggerException("POST /v1/trigger returned HTTP " + status + ": " + snippet, status);
        }
        if (resp.body() == null || resp.body().length == 0) {
            throw new CroniqTriggerException("POST /v1/trigger returned an empty body", status);
        }

        TriggerResponse parsed;
        try {
            parsed = json.readValue(resp.body(), TriggerResponse.class);
        } catch (IOException e) {
            throw new CroniqTriggerException("Could not parse /v1/trigger response", e);
        }
        return new TriggerResult(parsed.executionId(), parsed.queued(), parsed.deduplicated());
    }

    private static String buildAuthHeader(CroniqClientOptions options) {
        if (options.apiKey() != null && !options.apiKey().isBlank()) {
            return "ApiKey " + options.apiKey();
        }
        if (options.bearerToken() != null && !options.bearerToken().isBlank()) {
            return "Bearer " + options.bearerToken();
        }
        return null;
    }

    private URI resolve(String path) {
        String base = baseUrl.toString();
        if (base.endsWith("/")) {
            base = base.substring(0, base.length() - 1);
        }
        return URI.create(base + path);
    }

    private static String snippet(byte[] body) {
        if (body == null || body.length == 0) {
            return "";
        }
        String text = new String(body, StandardCharsets.UTF_8);
        return text.length() > BODY_SNIPPET_MAX ? text.substring(0, BODY_SNIPPET_MAX) + "…" : text;
    }

    private static String sdkVersion() {
        Package pkg = CroniqTriggerClient.class.getPackage();
        String v = pkg == null ? null : pkg.getImplementationVersion();
        return v != null ? v : "0.0.0-dev";
    }
}
