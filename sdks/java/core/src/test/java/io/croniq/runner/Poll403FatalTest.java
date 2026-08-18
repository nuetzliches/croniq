package io.croniq.runner;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import com.sun.net.httpserver.HttpServer;
import io.croniq.runner.config.CroniqRunnerOptions;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;

/**
 * A 403 on the poll endpoint is the ownership refusal from #436: the
 * credential is bound to a different {@code runner_id}. It is permanent, so
 * the runner must stop on the first one rather than retrying on the poll
 * interval (issue #437).
 */
class Poll403FatalTest {

    @Test
    void httpExceptionCarriesTheStatusCode() {
        // Before #437 the wire layer collapsed every non-2xx into a plain
        // IOException whose only record of the status was the message text.
        CroniqHttpException e = new CroniqHttpException("poll", 403, "denied");
        assertThat(e.statusCode()).isEqualTo(403);
        assertThat(e.operation()).isEqualTo("poll");
        assertThat(e.body()).isEqualTo("denied");
        assertThat(e.isOwnershipDenied()).isTrue();

        for (int status : new int[] {404, 409, 500, 503}) {
            assertThat(new CroniqHttpException("renew", status, "").isOwnershipDenied())
                    .as("HTTP %d must stay transient", status)
                    .isFalse();
        }
    }

    @Test
    void ownershipDeniedExceptionNamesRunnerIdAndRemedy() {
        var e = new CroniqOwnershipDeniedException("runner-42", null);
        assertThat(e.runnerId()).isEqualTo("runner-42");
        assertThat(e.getMessage()).contains("runner-42").contains("DELETE /v1/runners/{id}");
    }

    @Test
    void runStopsAfterASinglePollWhenTheServerAnswers403() throws Exception {
        AtomicInteger polls = new AtomicInteger();
        HttpServer server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/", exchange -> {
            if ("/v1/work/poll".equals(exchange.getRequestURI().getPath())) {
                polls.incrementAndGet();
            }
            byte[] body = "{\"error\":\"runner_id is bound to another credential\"}".getBytes(StandardCharsets.UTF_8);
            exchange.getResponseHeaders().add("Content-Type", "application/json");
            exchange.sendResponseHeaders(403, body.length);
            try (var os = exchange.getResponseBody()) {
                os.write(body);
            }
        });
        server.start();
        try {
            CroniqRunnerOptions options = CroniqRunnerOptions.builder()
                    .serverUrl("http://127.0.0.1:" + server.getAddress().getPort())
                    .runnerId("runner-denied")
                    .apiKey("croniq_testkey")
                    .pollTimeout(Duration.ofMillis(500))
                    .pollRetryDelay(Duration.ofMillis(50))
                    .drainTimeout(Duration.ofMillis(500))
                    .build();
            CroniqRunner runner = CroniqRunner.builder().options(options).build();

            assertThatThrownBy(runner::run)
                    .isInstanceOf(CroniqOwnershipDeniedException.class)
                    .hasMessageContaining("runner-denied");

            assertThat(polls.get()).as("403 is fatal — exactly one poll").isEqualTo(1);
        } finally {
            stop(server);
        }
    }

    private static void stop(HttpServer server) throws IOException {
        server.stop(0);
    }
}
