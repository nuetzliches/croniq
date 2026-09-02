package io.croniq.runner;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatExceptionOfType;
import static org.assertj.core.api.Assertions.assertThatIllegalArgumentException;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import io.croniq.runner.config.CroniqClientOptions;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CopyOnWriteArrayList;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/**
 * Wire-level coverage for {@link CroniqTriggerClient}: request shape
 * (snake_case, omission of unset optionals, auth header), response parsing
 * (including the forward-compatible {@code deduplicated} flag), and error
 * propagation. Mirrors the .NET SDK's {@code CroniqTriggerClientTests}; runs
 * the real client against a tiny in-process {@link HttpServer}.
 */
class CroniqTriggerClientTest {

    private static final ObjectMapper JSON = new ObjectMapper();

    private RecordingServer server;

    @BeforeEach
    void startServer() throws IOException {
        server = new RecordingServer();
    }

    @AfterEach
    void stopServer() {
        server.close();
    }

    @Test
    void postsSnakeCaseBodyToTriggerEndpoint() throws Exception {
        server.respond(200, "{\"execution_id\":\"exec-1\",\"queued\":3,\"deduplicated\":false}");
        CroniqTriggerClient client = client("croniq_trigger_key", null);

        TriggerResult result = client.trigger(TriggerRequest.builder("billing:invoice-generate")
                .metadata(Map.of("invoice_id", "inv_42"))
                .require(List.of("billing"))
                .prefer(List.of("eu-central"))
                .timeout("10m")
                .idempotencyKey("evt-123")
                .build());

        RecordingServer.Recorded req = server.last();
        assertThat(req.method()).isEqualTo("POST");
        assertThat(req.path()).isEqualTo("/v1/trigger");

        JsonNode body = JSON.readTree(req.body());
        assertThat(body.get("job_key").asText()).isEqualTo("billing:invoice-generate");
        assertThat(body.get("metadata").get("invoice_id").asText()).isEqualTo("inv_42");
        assertThat(body.get("require").get(0).asText()).isEqualTo("billing");
        assertThat(body.get("prefer").get(0).asText()).isEqualTo("eu-central");
        assertThat(body.get("timeout").asText()).isEqualTo("10m");
        assertThat(body.get("idempotency_key").asText()).isEqualTo("evt-123");

        assertThat(result.executionId()).isEqualTo("exec-1");
        assertThat(result.queued()).isEqualTo(3);
        assertThat(result.deduplicated()).isFalse();
    }

    @Test
    void omitsUnsetOptionalFields() throws Exception {
        server.respond(200, "{\"execution_id\":\"exec-1\",\"queued\":1}");
        CroniqTriggerClient client = client("croniq_trigger_key", null);

        client.trigger("etl:data-sync");

        JsonNode body = JSON.readTree(server.last().body());
        assertThat(body.get("job_key").asText()).isEqualTo("etl:data-sync");
        assertThat(body.has("metadata")).isFalse();
        assertThat(body.has("require")).isFalse();
        assertThat(body.has("prefer")).isFalse();
        assertThat(body.has("timeout")).isFalse();
        assertThat(body.has("idempotency_key")).isFalse();
    }

    @Test
    void omitsExplicitlyEmptyOptionalFields() throws Exception {
        // Issue #553: empty normalizes to absent. The server already reads an
        // empty `require` as "inherit the job's", so sending `[]` is a second
        // wire spelling of a message that has one -- and `timeout: ""` is not
        // a parseable duration, so honouring it would hand the runner a broken
        // value where omitting it inherits the job's own timeout.
        server.respond(200, "{\"execution_id\":\"exec-1\",\"queued\":1}");
        CroniqTriggerClient client = client("croniq_trigger_key", null);

        client.trigger(TriggerRequest.builder("etl:data-sync")
                .metadata(Map.of())
                .require(List.of())
                .prefer(List.of())
                .timeout("   ")
                .idempotencyKey("")
                .build());

        JsonNode body = JSON.readTree(server.last().body());
        assertThat(body.get("job_key").asText()).isEqualTo("etl:data-sync");
        assertThat(body.has("metadata")).isFalse();
        assertThat(body.has("require")).isFalse();
        assertThat(body.has("prefer")).isFalse();
        assertThat(body.has("timeout")).isFalse();
        assertThat(body.has("idempotency_key")).isFalse();
    }

    @Test
    void keepsNonEmptyOptionalFields() throws Exception {
        // The empty-normalization must not swallow real values.
        server.respond(200, "{\"execution_id\":\"exec-1\",\"queued\":1}");
        CroniqTriggerClient client = client("croniq_trigger_key", null);

        client.trigger(TriggerRequest.builder("etl:data-sync")
                .require(List.of("gpu"))
                .timeout(" 15m ")
                .build());

        JsonNode body = JSON.readTree(server.last().body());
        assertThat(body.get("require").get(0).asText()).isEqualTo("gpu");
        assertThat(body.get("timeout").asText()).isEqualTo("15m");
    }

    @Test
    void missingDeduplicatedFlagDefaultsToFalse() {
        // Older servers don't send `deduplicated` at all.
        server.respond(200, "{\"execution_id\":\"exec-1\",\"queued\":0}");
        CroniqTriggerClient client = client("croniq_trigger_key", null);

        TriggerResult result = client.trigger("etl:data-sync");

        assertThat(result.deduplicated()).isFalse();
    }

    @Test
    void deduplicatedFlagIsSurfaced() {
        server.respond(200, "{\"execution_id\":\"exec-1\",\"queued\":0,\"deduplicated\":true}");
        CroniqTriggerClient client = client("croniq_trigger_key", null);

        TriggerResult result = client.trigger(
                TriggerRequest.builder("etl:data-sync").idempotencyKey("evt-1").build());

        assertThat(result.deduplicated()).isTrue();
        assertThat(result.executionId()).isEqualTo("exec-1");
    }

    @Test
    void nonSuccessStatusThrows() {
        server.respond(404, "{\"error\":\"unknown job\"}");
        CroniqTriggerClient client = client("croniq_trigger_key", null);

        assertThatExceptionOfType(CroniqTriggerException.class)
                .isThrownBy(() -> client.trigger("nope:missing"))
                .satisfies(e -> {
                    assertThat(e.statusCode()).isEqualTo(404);
                    assertThat(e.isQueueOverflow()).isFalse();
                });
    }

    @Test
    void queueOverflowSurfacedAsError() {
        // Per-job queue-overflow backpressure (issue #299): 429 with an empty body.
        server.respond(429, "{\"execution_id\":\"\",\"queued\":0,\"deduplicated\":false}");
        CroniqTriggerClient client = client("croniq_trigger_key", null);

        assertThatExceptionOfType(CroniqTriggerException.class)
                .isThrownBy(() -> client.trigger("billing:invoice"))
                .satisfies(e -> {
                    assertThat(e.statusCode()).isEqualTo(429);
                    assertThat(e.isQueueOverflow()).isTrue();
                });
    }

    @Test
    void blankJobKeyThrowsWithoutSendingRequest() {
        server.respond(200, "{\"execution_id\":\"x\",\"queued\":0}");
        CroniqTriggerClient client = client("croniq_trigger_key", null);

        assertThatIllegalArgumentException().isThrownBy(() -> client.trigger("  "));
        assertThat(server.recorded()).isEmpty();
    }

    @Test
    void apiKeyAuthHeaderSentAsApiKeyScheme() {
        server.respond(200, "{\"execution_id\":\"exec-1\",\"queued\":1}");
        CroniqTriggerClient client = client("croniq_producer_key", null);

        client.trigger("billing:invoice");

        assertThat(server.last().header("authorization")).isEqualTo("ApiKey croniq_producer_key");
    }

    @Test
    void bearerTokenUsedWhenNoApiKey() {
        server.respond(200, "{\"execution_id\":\"exec-1\",\"queued\":1}");
        CroniqTriggerClient client = client(null, "tok-123");

        client.trigger("billing:invoice");

        assertThat(server.last().header("authorization")).isEqualTo("Bearer tok-123");
    }

    @Test
    void apiKeyTakesPrecedenceOverBearerToken() {
        server.respond(200, "{\"execution_id\":\"exec-1\",\"queued\":1}");
        CroniqTriggerClient client = client("the-key", "the-token");

        client.trigger("billing:invoice");

        assertThat(server.last().header("authorization")).isEqualTo("ApiKey the-key");
    }

    private CroniqTriggerClient client(String apiKey, String bearerToken) {
        return new CroniqTriggerClient(CroniqClientOptions.builder()
                .serverUrl(server.baseUrl())
                .apiKey(apiKey)
                .bearerToken(bearerToken)
                .build());
    }

    /** Minimal in-process HTTP server that records requests and replays a fixed response. */
    private static final class RecordingServer implements AutoCloseable {

        private final HttpServer http;
        private final List<Recorded> recorded = new CopyOnWriteArrayList<>();
        private volatile int status = 200;
        private volatile String responseBody = "{}";

        RecordingServer() throws IOException {
            http = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
            http.createContext("/", this::handle);
            http.start();
        }

        void respond(int status, String body) {
            this.status = status;
            this.responseBody = body;
        }

        String baseUrl() {
            return "http://127.0.0.1:" + http.getAddress().getPort();
        }

        List<Recorded> recorded() {
            return recorded;
        }

        Recorded last() {
            assertThat(recorded).isNotEmpty();
            return recorded.get(recorded.size() - 1);
        }

        private void handle(HttpExchange ex) throws IOException {
            String body;
            try (InputStream in = ex.getRequestBody()) {
                body = new String(in.readAllBytes(), StandardCharsets.UTF_8);
            }
            recorded.add(
                    new Recorded(ex.getRequestMethod(), ex.getRequestURI().getPath(), ex.getRequestHeaders(), body));
            byte[] payload = responseBody.getBytes(StandardCharsets.UTF_8);
            ex.getResponseHeaders().add("Content-Type", "application/json");
            ex.sendResponseHeaders(status, payload.length == 0 ? -1 : payload.length);
            try (OutputStream out = ex.getResponseBody()) {
                out.write(payload);
            }
        }

        @Override
        public void close() {
            http.stop(0);
        }

        record Recorded(String method, String path, com.sun.net.httpserver.Headers headers, String body) {
            String header(String name) {
                return headers.getFirst(name);
            }
        }
    }
}
