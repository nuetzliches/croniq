package io.croniq.runner;

import io.croniq.runner.config.CroniqRunnerOptions;
import io.croniq.runner.handler.CroniqJobHandler;
import io.croniq.runner.internal.CroniqClient;
import io.croniq.runner.internal.ExecutionDispatcher;
import io.croniq.runner.internal.HandlerRegistry;
import io.croniq.runner.internal.RunnerIdentityResolver;
import io.croniq.runner.protocol.PollRequest;
import io.croniq.runner.protocol.PollResponse;
import java.time.Duration;
import java.util.Objects;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.atomic.AtomicBoolean;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Entry point for the Croniq Java SDK. Polls work, dispatches handlers on
 * virtual threads, and reports completion.
 *
 * <p>Construct via {@link #builder()}, then call {@link #run()} from your
 * application's main thread. Call {@link #close()} from another thread to
 * stop the loop and release the handler executor. An async/managed lifecycle
 * lands in PR-3.
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
        this.dispatcher = new ExecutionDispatcher(client, registry, handlerExecutor, options, runnerId);
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

    @Override
    public void close() {
        if (stopped.compareAndSet(false, true)) {
            Thread t = runThread;
            if (t != null) {
                t.interrupt();
            }
            handlerExecutor.shutdownNow();
        }
    }

    public static final class Builder {

        private CroniqRunnerOptions options = CroniqRunnerOptions.builder().build();
        private final HandlerRegistry.Builder registryBuilder = HandlerRegistry.builder();
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

        public Builder defaultHandler(CroniqJobHandler handler) {
            registryBuilder.defaultHandler(handler);
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
