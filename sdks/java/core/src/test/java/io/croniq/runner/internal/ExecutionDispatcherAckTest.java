package io.croniq.runner.internal;

import static org.assertj.core.api.Assertions.assertThat;

import io.croniq.runner.CroniqHttpException;
import io.croniq.runner.config.CroniqRunnerOptions;
import io.croniq.runner.handler.CroniqJobHandler;
import io.croniq.runner.protocol.AckRequest;
import io.croniq.runner.protocol.RenewRequest;
import io.croniq.runner.protocol.WorkAssignment;
import io.croniq.runner.protocol.WorkEvent;
import java.io.IOException;
import java.net.ConnectException;
import java.net.http.HttpTimeoutException;
import java.time.Duration;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

/**
 * Ack retry (review 2026-10-24, K-1), ack-before-slot ordering and the
 * interrupt race (K-5), and re-delivery dedupe (K-11).
 */
class ExecutionDispatcherAckTest {

    private static final String EXEC = "exec-1";

    private final ExecutorService executor = Executors.newVirtualThreadPerTaskExecutor();

    @AfterEach
    void tearDown() {
        executor.shutdownNow();
    }

    /** Scripted ack outcomes: each call pops the next one; null means success. */
    private static final class FakeClient extends CroniqClient {
        final List<AckRequest> acks = new CopyOnWriteArrayList<>();
        final List<Object> script;
        volatile AckHook hook;

        FakeClient(Object... script) {
            super(options());
            this.script = new CopyOnWriteArrayList<>(script);
        }

        @Override
        public void ack(AckRequest request) throws IOException, InterruptedException {
            acks.add(request);
            if (hook != null) {
                hook.onAck();
            }
            if (script.isEmpty()) {
                return;
            }
            Object next = script.remove(0);
            if (next instanceof IOException e) {
                throw e;
            }
        }

        @Override
        public void renew(RenewRequest request) {}

        @Override
        public void pushEvents(String executionId, List<WorkEvent> events) {}
    }

    @FunctionalInterface
    private interface AckHook {
        void onAck() throws IOException, InterruptedException;
    }

    private static CroniqRunnerOptions options() {
        return CroniqRunnerOptions.builder()
                .serverUrl("http://127.0.0.1:1")
                .runnerId("runner-test")
                .requestTimeout(Duration.ofMillis(500))
                .renewInterval(Duration.ofSeconds(30))
                .build();
    }

    private ExecutionDispatcher dispatcher(FakeClient client, CroniqJobHandler handler) {
        var registry = new HandlerRegistry(Map.of("job", handler), Map.of(), null);
        var d = new ExecutionDispatcher(client, registry, executor, options(), "runner-test", List.of());
        d.ackRetryDelays = List.of(Duration.ofMillis(10), Duration.ofMillis(20));
        return d;
    }

    private static WorkAssignment work() {
        return new WorkAssignment(EXEC, "job", "2026-10-24T10:00:00Z", null, 1, null, "5m");
    }

    private static void awaitIdle(ExecutionDispatcher d) throws InterruptedException {
        long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(5);
        while (d.inflightCount() > 0 && System.nanoTime() < deadline) {
            Thread.sleep(10);
        }
        assertThat(d.inflightCount()).as("execution left the in-flight set").isZero();
    }

    @Test
    void ackAnswered503IsRetriedUntilItSucceeds() throws Exception {
        var client = new FakeClient(new CroniqHttpException("ack", 503, ""));
        var d = dispatcher(client, ctx -> {});
        d.dispatch(work());
        awaitIdle(d);
        assertThat(client.acks).hasSize(2);
        assertThat(client.acks).allSatisfy(a -> assertThat(a.status()).isEqualTo("success"));
    }

    @Test
    void ackAnswered4xxIsNotRetried() throws Exception {
        var client = new FakeClient(new CroniqHttpException("ack", 409, ""), new CroniqHttpException("ack", 409, ""));
        var d = dispatcher(client, ctx -> {});
        d.dispatch(work());
        awaitIdle(d);
        Thread.sleep(100); // past every retry delay
        assertThat(client.acks).hasSize(1);
    }

    @Test
    void ackRetriesStopAfterThreeAttempts() throws Exception {
        var client = new FakeClient(
                new CroniqHttpException("ack", 500, ""),
                new CroniqHttpException("ack", 502, ""),
                new CroniqHttpException("ack", 503, ""),
                new CroniqHttpException("ack", 503, ""));
        var d = dispatcher(client, ctx -> {});
        d.dispatch(work());
        awaitIdle(d);
        Thread.sleep(100);
        assertThat(client.acks).hasSize(3);
    }

    @Test
    void transportErrorsAndTimeoutsAreRetried() throws Exception {
        var client = new FakeClient(new ConnectException("connection reset"), new HttpTimeoutException("timed out"));
        var d = dispatcher(client, ctx -> {});
        d.dispatch(work());
        awaitIdle(d);
        assertThat(client.acks).hasSize(3);
    }

    @Test
    void ownershipDenialIsNotRetried() throws Exception {
        var client = new FakeClient(new CroniqHttpException("ack", 403, ""));
        var d = dispatcher(client, ctx -> {});
        d.dispatch(work());
        awaitIdle(d);
        Thread.sleep(100);
        assertThat(client.acks).hasSize(1);
    }

    @Test
    void executionStaysInFlightUntilItsAckIsSent() throws Exception {
        var client = new FakeClient();
        var seenDuringAck = new CopyOnWriteArrayList<Set<String>>();
        var d = dispatcher(client, ctx -> {});
        client.hook = () -> seenDuringAck.add(d.inflightIds());
        d.dispatch(work());
        awaitIdle(d);
        // Before K-5 the slot was freed first, so the poll loop reported an
        // empty in-flight list and claimed new work while the ack was on the wire.
        assertThat(seenDuringAck).containsExactly(Set.of(EXEC));
    }

    @Test
    void staleInterruptFromLateCancelDoesNotKillTheAck() throws Exception {
        // A server cancel that lands after the handler returned leaves the
        // worker's interrupt flag set; HttpClient.send then throws
        // InterruptedException and the ack was lost.
        var client = new FakeClient();
        var interruptedAtAck = new AtomicInteger();
        client.hook = () -> {
            if (Thread.currentThread().isInterrupted()) {
                interruptedAtAck.incrementAndGet();
                throw new InterruptedException("HttpClient.send observed the interrupt");
            }
        };
        var d = dispatcher(client, ctx -> Thread.currentThread().interrupt());
        d.dispatch(work());
        awaitIdle(d);
        assertThat(interruptedAtAck).hasValue(0);
        assertThat(client.acks).hasSize(1);
    }

    @Test
    void cancelArrivingDuringTheAckDoesNotInterruptIt() throws Exception {
        var client = new FakeClient();
        var ackStarted = new CountDownLatch(1);
        var cancelSent = new CountDownLatch(1);
        var interruptedAtAck = new AtomicInteger();
        client.hook = () -> {
            ackStarted.countDown();
            cancelSent.await(5, TimeUnit.SECONDS);
            if (Thread.currentThread().isInterrupted()) {
                interruptedAtAck.incrementAndGet();
            }
        };
        var d = dispatcher(client, ctx -> {});
        d.dispatch(work());
        assertThat(ackStarted.await(5, TimeUnit.SECONDS)).isTrue();
        d.cancel(EXEC); // e.g. the drain's hard-cancel
        cancelSent.countDown();
        awaitIdle(d);
        assertThat(interruptedAtAck).hasValue(0);
        assertThat(client.acks).hasSize(1);
    }

    @Test
    void reDeliveredExecutionIdIsNotDispatchedAgain() throws Exception {
        var client = new FakeClient();
        var release = new CountDownLatch(1);
        var runs = new AtomicInteger();
        var d = dispatcher(client, ctx -> {
            runs.incrementAndGet();
            release.await(5, TimeUnit.SECONDS);
        });
        d.dispatch(work());
        d.dispatch(work());
        assertThat(d.inflightCount()).isEqualTo(1);
        release.countDown();
        awaitIdle(d);
        Thread.sleep(50);
        assertThat(runs).hasValue(1);
        assertThat(client.acks).hasSize(1);
    }

    @Test
    void maxAckDurationCoversEveryAttemptAndDelay() {
        var d = new ExecutionDispatcher(
                new FakeClient(), new HandlerRegistry(Map.of(), Map.of(), null), executor, options(), "r", List.of());
        // 3 × 500 ms request timeout + 1 s + 2 s.
        assertThat(d.maxAckDuration()).isEqualTo(Duration.ofMillis(4500));
    }
}
