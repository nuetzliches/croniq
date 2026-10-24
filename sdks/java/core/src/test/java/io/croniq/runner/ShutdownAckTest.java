package io.croniq.runner;

import static org.assertj.core.api.Assertions.assertThat;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import io.croniq.runner.config.CroniqRunnerOptions;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.Test;

/**
 * Shutdown after the drain timeout still delivers the failure ack (review
 * 2026-10-24, K-4), and the poll retry delay is jittered (K-9).
 */
class ShutdownAckTest {

    @Test
    void failureAckAndItsRetrySurviveTheDrainTimeout() throws Exception {
        AtomicInteger polls = new AtomicInteger();
        List<String> acks = new CopyOnWriteArrayList<>();
        HttpServer server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/", exchange -> {
            String path = exchange.getRequestURI().getPath();
            String body = new String(exchange.getRequestBody().readAllBytes(), StandardCharsets.UTF_8);
            switch (path) {
                case "/v1/work/poll" -> {
                    if (polls.incrementAndGet() == 1) {
                        respond(
                                exchange,
                                200,
                                "{\"work\":[{\"execution_id\":\"exec-stuck\",\"job_key\":\"stuck\","
                                        + "\"fire_at\":\"2026-10-24T10:00:00Z\",\"attempt\":1,\"metadata\":{},"
                                        + "\"timeout\":\"5m\"}],\"cancel\":[]}");
                    } else {
                        respond(exchange, 200, "{\"work\":[],\"cancel\":[]}");
                    }
                }
                case "/v1/work/ack" -> {
                    acks.add(body);
                    if (acks.size() == 1) {
                        // Slow 503: the retry lands past the 2 s after which
                        // close() used to call shutdownNow() unconditionally.
                        sleepQuietly(1500);
                        respond(exchange, 503, "{\"error\":\"unavailable\"}");
                    } else {
                        respond(exchange, 200, "{}");
                    }
                }
                default -> respond(exchange, 200, "{}");
            }
        });
        server.setExecutor(java.util.concurrent.Executors.newVirtualThreadPerTaskExecutor());
        server.start();
        try {
            CroniqRunnerOptions options = CroniqRunnerOptions.builder()
                    .serverUrl("http://127.0.0.1:" + server.getAddress().getPort())
                    .runnerId("runner-drain")
                    .apiKey("croniq_testkey")
                    .maxInflight(1)
                    .pollTimeout(Duration.ofMillis(500))
                    .requestTimeout(Duration.ofSeconds(3))
                    .pollRetryDelay(Duration.ofMillis(50))
                    .capacityBackoff(Duration.ofMillis(50))
                    .drainTimeout(Duration.ofMillis(200))
                    .build();
            CountDownLatch started = new CountDownLatch(1);
            CroniqRunner runner = CroniqRunner.builder()
                    .options(options)
                    .addJob("stuck", ctx -> {
                        started.countDown();
                        Thread.sleep(10_000); // unwinds on the hard-cancel interrupt
                    })
                    .build();
            Thread runThread = Thread.ofPlatform().start(() -> {
                try {
                    runner.run();
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                }
            });
            assertThat(started.await(5, TimeUnit.SECONDS)).isTrue();

            runner.close();
            runThread.join(5000);

            assertThat(acks).hasSize(2);
            assertThat(acks).allSatisfy(b -> assertThat(b)
                    .contains("\"execution_id\":\"exec-stuck\"")
                    .contains("\"status\":\"failure\""));
        } finally {
            server.stop(0);
        }
    }

    @Test
    void pollRetryDelayIsJitteredWithinTwentyPercent() {
        Duration base = Duration.ofMillis(1000);
        Set<Long> seen = new HashSet<>();
        for (int i = 0; i < 500; i++) {
            long ms = CroniqRunner.jittered(base).toMillis();
            assertThat(ms).isBetween(800L, 1200L);
            seen.add(ms);
        }
        assertThat(seen).as("delays are spread, not fixed").hasSizeGreaterThan(10);
        assertThat(CroniqRunner.jittered(Duration.ZERO)).isEqualTo(Duration.ZERO);
    }

    private static void respond(HttpExchange exchange, int status, String body) throws IOException {
        byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().add("Content-Type", "application/json");
        exchange.sendResponseHeaders(status, bytes.length);
        try (var os = exchange.getResponseBody()) {
            os.write(bytes);
        }
    }

    private static void sleepQuietly(long ms) {
        try {
            Thread.sleep(ms);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
    }
}
