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
 * A 409 on the poll endpoint is the fencing refusal from #374: a newer
 * instance has taken this {@code runner_id} over. A single one is transient —
 * the deposed instance may win the identity back — and conformance case 11
 * pins that it is retried. A <em>streak</em> of them is a duplicate
 * deployment, two processes started with the same fixed {@code runner_id},
 * and retrying that forever leaves the misconfiguration behind a warning that
 * scrolls past (issue #134 sub-item 1).
 */
class Poll409CeilingTest {

    @Test
    void httpExceptionClassifiesOnlyA409AsAnInstanceConflict() {
        assertThat(new CroniqHttpException("poll", 409, "conflict").isInstanceConflict())
                .isTrue();
        for (int status : new int[] {403, 404, 500, 503}) {
            assertThat(new CroniqHttpException("poll", status, "").isInstanceConflict())
                    .as("HTTP %d is not an instance conflict", status)
                    .isFalse();
        }
    }

    @Test
    void conflictExceptionNamesRunnerIdStreakAndRemedy() {
        var e = new CroniqPollInstanceConflictException("runner-42", 3, null);
        assertThat(e.runnerId()).isEqualTo("runner-42");
        assertThat(e.consecutiveCount()).isEqualTo(3);
        assertThat(e.getMessage()).contains("runner-42").contains("rotate the runner_id");
    }

    @Test
    void ceilingIsRangeChecked() {
        // 0 would make the runner exit on its very first 409, which reads as a
        // crash-loop rather than the duplicate deployment it actually is.
        assertThatThrownBy(() -> CroniqRunnerOptions.builder().maxConsecutivePollConflicts(0))
                .isInstanceOf(IllegalArgumentException.class)
                .hasMessageContaining("maxConsecutivePollConflicts");
    }

    @Test
    void runStopsOnceTheStreakExhaustsTheCeiling() throws Exception {
        AtomicInteger polls = new AtomicInteger();
        HttpServer server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/", exchange -> {
            if ("/v1/work/poll".equals(exchange.getRequestURI().getPath())) {
                polls.incrementAndGet();
            }
            byte[] body = "{\"error\":\"runner instance conflict\"}".getBytes(StandardCharsets.UTF_8);
            exchange.getResponseHeaders().add("Content-Type", "application/json");
            exchange.sendResponseHeaders(409, body.length);
            try (var os = exchange.getResponseBody()) {
                os.write(body);
            }
        });
        server.start();
        try {
            CroniqRunnerOptions options = CroniqRunnerOptions.builder()
                    .serverUrl("http://127.0.0.1:" + server.getAddress().getPort())
                    .runnerId("runner-duplicate")
                    .apiKey("croniq_testkey")
                    .pollTimeout(Duration.ofMillis(500))
                    .pollRetryDelay(Duration.ofMillis(20))
                    .drainTimeout(Duration.ofMillis(500))
                    .maxConsecutivePollConflicts(3)
                    .build();
            CroniqRunner runner = CroniqRunner.builder().options(options).build();

            assertThatThrownBy(runner::run)
                    .isInstanceOf(CroniqPollInstanceConflictException.class)
                    .hasMessageContaining("runner-duplicate");

            assertThat(polls.get())
                    .as("the runner stops at the configured ceiling")
                    .isEqualTo(3);
        } finally {
            stop(server);
        }
    }

    @Test
    void streakResetsOnANon409Failure() throws Exception {
        // Only *consecutive* conflicts count: the 500 in between is unrelated
        // to instance ownership, so an unlucky mix of failures must not add up
        // to a fatal error.
        int[] statuses = {409, 500, 409, 200};
        AtomicInteger polls = new AtomicInteger();
        HttpServer server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/", exchange -> {
            int n = polls.getAndIncrement();
            int status = statuses[Math.min(n, statuses.length - 1)];
            String json = status == 200 ? "{\"work\":[],\"cancel\":[]}" : "{\"error\":\"nope\"}";
            byte[] body = json.getBytes(StandardCharsets.UTF_8);
            exchange.getResponseHeaders().add("Content-Type", "application/json");
            exchange.sendResponseHeaders(status, body.length);
            try (var os = exchange.getResponseBody()) {
                os.write(body);
            }
        });
        server.start();
        CroniqRunner runner = null;
        try {
            CroniqRunnerOptions options = CroniqRunnerOptions.builder()
                    .serverUrl("http://127.0.0.1:" + server.getAddress().getPort())
                    .runnerId("runner-flaky")
                    .apiKey("croniq_testkey")
                    .pollTimeout(Duration.ofMillis(500))
                    .pollRetryDelay(Duration.ofMillis(20))
                    .drainTimeout(Duration.ofMillis(500))
                    .maxConsecutivePollConflicts(2)
                    .build();
            runner = CroniqRunner.builder().options(options).build();
            CroniqRunner started = runner;

            java.util.concurrent.atomic.AtomicReference<Throwable> thrown =
                    new java.util.concurrent.atomic.AtomicReference<>();
            Thread t = new Thread(() -> {
                try {
                    started.run();
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                } catch (RuntimeException e) {
                    thrown.set(e);
                }
            });
            t.start();

            long deadline = System.nanoTime() + Duration.ofSeconds(5).toNanos();
            while (polls.get() < 4 && System.nanoTime() < deadline) {
                Thread.sleep(10);
            }
            started.close();
            t.join(Duration.ofSeconds(5).toMillis());

            assertThat(thrown.get())
                    .as("the runner survived 409/500/409 with a ceiling of 2")
                    .isNull();
            assertThat(t.isAlive()).as("run() returned after close()").isFalse();
            assertThat(polls.get()).isGreaterThanOrEqualTo(4);
        } finally {
            if (runner != null) {
                runner.close();
            }
            stop(server);
        }
    }

    private static void stop(HttpServer server) throws IOException {
        server.stop(0);
    }
}
