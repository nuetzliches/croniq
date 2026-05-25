package io.croniq.runner;

import io.croniq.runner.config.CroniqRunnerOptions;
import io.croniq.runner.handler.CroniqJobHandler;
import io.croniq.runner.handler.CroniqRunnerObserver;
import io.croniq.runner.internal.CroniqClient;
import io.croniq.runner.internal.ExecutionDispatcher;
import io.croniq.runner.internal.HandlerRegistry;
import io.croniq.runner.internal.RunnerIdentityResolver;
import io.croniq.runner.protocol.PollRequest;
import io.croniq.runner.protocol.PollResponse;
import io.croniq.runner.protocol.RegisterJobRequest;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.Objects;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Entry point for the Croniq Java SDK. Polls work, dispatches handlers on
 * virtual threads, and reports completion.
 *
 * <p>Construct via {@link #builder()}, then call {@link #run()} from your
 * application's main thread. Call {@link #close()} from another thread to
 * stop the loop; the call blocks until in-flight executions drain (bounded
 * by {@link CroniqRunnerOptions#drainTimeout()}).
 *
 * <p>Concurrency model: the poll loop runs on a single platform thread.
 * Each work item is dispatched to a fresh virtual thread, so handlers can
 * freely block on I/O without consuming OS threads.
 */
public final class CroniqRunner implements AutoCloseable {

    private static final Logger log = LoggerFactory.getLogger(CroniqRunner.class);

    private final CroniqRunnerOptions options;
    private final HandlerRegistry registry;
    private final CroniqClient client;
    private final String runnerId;
    private final ExecutorService handlerExecutor;
    private final ExecutionDispatcher dispatcher;
    private final AtomicBoolean stopped = new AtomicBoolean(false);
    private volatile Thread runThread;

    private CroniqRunner(Builder b) {
        this.options = Objects.requireNonNull(b.options, "options");
        this.registry = b.registryBuilder.build();
        this.client = b.clientOverride != null ? b.clientOverride : new CroniqClient(options);
        this.runnerId = new RunnerIdentityResolver(options).resolve();
        this.handlerExecutor = Executors.newVirtualThreadPerTaskExecutor();
        this.dispatcher = new ExecutionDispatcher(client, registry, handlerExecutor, options, runnerId, b.observers);
    }

    /** SDK version baked in by Gradle's {@code Implementation-Version} jar attribute. */
    public static String sdkVersion() {
        Package pkg = CroniqRunner.class.getPackage();
        String v = pkg == null ? null : pkg.getImplementationVersion();
        return v != null ? v : "0.0.0-dev";
    }

    public String runnerId() {
        return runnerId;
    }

    public static Builder builder() {
        return new Builder();
    }

    /**
     * Drive the poll/dispatch loop on the calling thread until {@link #close()}
     * is invoked from another thread, or the thread is interrupted.
     */
    public void run() throws InterruptedException {
        runThread = Thread.currentThread();
        log.info(
                "Croniq runner {} started (server={}, capabilities={})",
                runnerId,
                options.serverUrl(),
                options.capabilities());
        selfRegister();
        try {
            while (!stopped.get()) {
                int slotsFree = options.maxInflight() - dispatcher.inflightCount();
                if (slotsFree <= 0) {
                    sleep(options.capacityBackoff());
                    continue;
                }
                PollResponse response;
                try {
                    PollRequest request = new PollRequest(
                            runnerId,
                            options.capabilities(),
                            slotsFree,
                            java.util.List.copyOf(dispatcher.inflightIds()),
                            null,
                            options.tags());
                    response = client.poll(request, options.pollTimeout());
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    break;
                } catch (Exception e) {
                    log.debug("Poll failed: {} — backing off {}", e.toString(), options.pollRetryDelay());
                    sleep(options.pollRetryDelay());
                    continue;
                }
                if (response != null && response.cancel() != null) {
                    for (String id : response.cancel()) {
                        dispatcher.cancel(id);
                    }
                }
                if (response != null && response.work() != null) {
                    for (var work : response.work()) {
                        dispatcher.dispatch(work);
                    }
                }
            }
        } finally {
            runThread = null;
            log.info("Croniq runner {} stopping", runnerId);
        }
    }

    private static void sleep(Duration d) throws InterruptedException {
        long ms = d == null ? 0 : Math.max(0, d.toMillis());
        if (ms > 0) {
            Thread.sleep(ms);
        }
    }

    /**
     * POST a {@code /v1/jobs/register} for every handler that was registered
     * with a schedule. Best-effort: failures are logged and swallowed —
     * registration is idempotent so the next runner start retries naturally.
     */
    private void selfRegister() {
        for (var entry : registry.scheduled()) {
            var request = new RegisterJobRequest(
                    entry.jobKey(), entry.schedule(), null, null, runnerId, options.capabilities(), null);
            try {
                client.registerJob(request);
                log.info("Self-registered job {} with schedule {}", entry.jobKey(), entry.schedule());
            } catch (Exception e) {
                log.warn("Self-register failed for {}: {}", entry.jobKey(), e.toString());
            }
        }
    }

    /**
     * Stop polling and drain in-flight executions before returning. Mirrors the
     * .NET SDK's {@code StopAsync(CancellationToken)} semantics:
     *
     * <ol>
     *   <li>Set the stop flag so the poll loop exits at its next checkpoint.
     *   <li>Interrupt the poll thread so it returns immediately even if mid-poll.
     *   <li>Wait up to {@link CroniqRunnerOptions#drainTimeout()} for in-flight
     *       handlers to complete naturally — they are NOT interrupted during
     *       drain. Server-initiated cancels via {@code PollResponse.cancel}
     *       are still honoured because the poll loop already stopped.
     *   <li>If drain times out with handlers still running, force-cancel them
     *       (sets the cancellation flag and interrupts the worker threads).
     * </ol>
     *
     * <p>Idempotent — subsequent calls return immediately.
     */
    @Override
    public void close() {
        if (!stopped.compareAndSet(false, true)) {
            return;
        }
        Thread t = runThread;
        if (t != null) {
            t.interrupt();
        }

        long drainNanos = options.drainTimeout().toNanos();
        long deadline = System.nanoTime() + drainNanos;
        while (dispatcher.inflightCount() > 0) {
            long remainingMs = TimeUnit.NANOSECONDS.toMillis(deadline - System.nanoTime());
            if (remainingMs <= 0) {
                int n = dispatcher.inflightCount();
                if (n > 0) {
                    log.warn(
                            "Drain timeout ({}ms) elapsed with {} execution(s) still in-flight — cancelling",
                            options.drainTimeout().toMillis(),
                            n);
                }
                break;
            }
            try {
                Thread.sleep(Math.min(50, remainingMs));
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                break;
            }
        }
        // Force-cancel anything that didn't drain naturally — sets the
        // cancellation flag AND interrupts each worker so blocking I/O
        // unwinds promptly. After this, shutdownNow gives the executor a
        // chance to release any held threads.
        for (String id : dispatcher.inflightIds()) {
            dispatcher.cancel(id);
        }
        handlerExecutor.shutdown();
        try {
            if (!handlerExecutor.awaitTermination(2, TimeUnit.SECONDS)) {
                handlerExecutor.shutdownNow();
            }
        } catch (InterruptedException e) {
            handlerExecutor.shutdownNow();
            Thread.currentThread().interrupt();
        }
    }

    public static final class Builder {

        private CroniqRunnerOptions options = CroniqRunnerOptions.builder().build();
        private final HandlerRegistry.Builder registryBuilder = HandlerRegistry.builder();
        private final List<CroniqRunnerObserver> observers = new ArrayList<>();
        private CroniqClient clientOverride;

        private Builder() {}

        public Builder options(CroniqRunnerOptions options) {
            this.options = Objects.requireNonNull(options, "options");
            return this;
        }

        public Builder addJob(String jobKey, CroniqJobHandler handler) {
            registryBuilder.add(jobKey, handler);
            return this;
        }

        /**
         * Register a handler with a server-side schedule. The runner calls
         * {@code POST /v1/jobs/register} at startup for every job registered
         * this way — the server then drives execution via the regular poll
         * loop. Schedule format follows the Croniqfile DSL ({@code "5m"},
         * {@code "*\/15 * * * *"}, etc.).
         */
        public Builder addJob(String jobKey, String schedule, CroniqJobHandler handler) {
            registryBuilder.add(jobKey, schedule, handler);
            return this;
        }

        public Builder defaultHandler(CroniqJobHandler handler) {
            registryBuilder.defaultHandler(handler);
            return this;
        }

        /**
         * Register an observer that receives {@code onExecutionStart} /
         * {@code onExecutionEnd} callbacks around every execution. Multiple
         * observers can be registered; they are invoked in registration order.
         * Exceptions thrown by observers are logged and swallowed — observability
         * never blocks job dispatch.
         */
        public Builder observer(CroniqRunnerObserver observer) {
            this.observers.add(Objects.requireNonNull(observer, "observer"));
            return this;
        }

        /**
         * Test/conformance escape hatch — swap in a pre-built {@link CroniqClient}
         * (e.g., pointed at a WireMock instance). Package-internal callers in the
         * conformance binding rely on this; downstream users should ignore it.
         */
        public Builder clientForTesting(CroniqClient client) {
            this.clientOverride = client;
            return this;
        }

        public CroniqRunner build() {
            return new CroniqRunner(this);
        }
    }
}
