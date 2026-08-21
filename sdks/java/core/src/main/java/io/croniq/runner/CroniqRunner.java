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
        // Consecutive 409 Conflict responses on poll. Reset by a successful poll
        // or by any non-409 failure — see maxConsecutivePollConflicts().
        int consecutiveConflicts = 0;
        // Consecutive 401s, tracked separately: a run of conflicts must not spend
        // the auth budget, or a duplicate deployment would be reported as an
        // authentication failure.
        int consecutiveAuthFailures = 0;
        try {
            while (!stopped.get()) {
                // Control-slot polling (issue #176): even at capacity we
                // still poll so the server can deliver cancels via
                // PollResponse.cancel. We send the runner's full
                // maxInflight() and the current inflight list — the server
                // computes capacity = max - inflight.size() and returns
                // immediately when zero. capacityBackoff() paces the loop
                // and prevents a stampede after this at-capacity iteration.
                int slotsFree = options.maxInflight() - dispatcher.inflightCount();
                boolean atCapacity = slotsFree <= 0;
                PollResponse response;
                try {
                    PollRequest request = new PollRequest(
                            runnerId,
                            options.capabilities(),
                            options.maxInflight(),
                            java.util.List.copyOf(dispatcher.inflightIds()),
                            null,
                            options.tags());
                    response = client.poll(request, options.pollTimeout());
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    break;
                } catch (CroniqHttpException e) {
                    // A 403 is permanent (issue #437): the credential is bound
                    // to another runner_id, so the next poll fails identically.
                    // Stop with an actionable error instead of retrying on the
                    // poll interval, which makes a fenced-out runner look idle.
                    if (e.isOwnershipDenied()) {
                        log.error(
                                "fatal: poll refused with 403 Forbidden — this runner's credential does not own"
                                        + " runner_id {}. Give the runner its own runner_id, or release the existing"
                                        + " binding with DELETE /v1/runners/{id}",
                                runnerId);
                        throw new CroniqOwnershipDeniedException(runnerId, e);
                    }
                    // A 401 says the key was rejected, and the client never re-reads
                    // it, so every later poll presents the same dead credential.
                    // Budgeted rather than fatal at once: rotation hands over
                    // through an expiry window (server issue #471) and a race
                    // around it should not kill a healthy runner (issue #473).
                    if (e.isUnauthorized()) {
                        consecutiveAuthFailures++;
                        if (consecutiveAuthFailures >= options.maxConsecutiveAuthFailures()) {
                            log.error(
                                    "fatal: poll refused with 401 Unauthorized {} times in a row — the API key"
                                            + " was rejected. It may have been revoked, or its rotation grace"
                                            + " window may have elapsed. Restart the runner with the current key",
                                    consecutiveAuthFailures);
                            throw new CroniqAuthFailedException(consecutiveAuthFailures, e);
                        }
                        log.warn(
                                "Poll returned 401 Unauthorized ({}/{}) — the API key was rejected;"
                                        + " retrying after {}",
                                consecutiveAuthFailures,
                                options.maxConsecutiveAuthFailures(),
                                options.pollRetryDelay());
                        // A 401 is not a 409, so it clears the conflict budget just
                        // like any other non-409 failure. This branch continues
                        // before reaching the reset below, so it has to do it here
                        // (issue #508).
                        consecutiveConflicts = 0;
                        sleep(options.pollRetryDelay());
                        continue;
                    }
                    // Anything that is not a 401 clears the auth budget: a 5xx or a
                    // timeout says nothing about whether the credential is valid.
                    consecutiveAuthFailures = 0;
                    // A 409 means a newer instance has taken this runner_id over
                    // (fencing, issue #374). One is transient — the deposed
                    // instance may win it back — so we back off and retry. A
                    // streak of them is a duplicate deployment, and retrying
                    // forever hides it behind a warning that scrolls past
                    // (issue #134 sub-item 1).
                    if (e.isInstanceConflict()) {
                        consecutiveConflicts++;
                        if (consecutiveConflicts >= options.maxConsecutivePollConflicts()) {
                            log.error(
                                    "fatal: poll refused with 409 Conflict {} times in a row — another runner is"
                                            + " registered with runner_id {}. Stop the duplicate process or rotate"
                                            + " the runner_id",
                                    consecutiveConflicts,
                                    runnerId);
                            throw new CroniqPollInstanceConflictException(runnerId, consecutiveConflicts, e);
                        }
                        log.warn(
                                "Poll returned 409 Conflict ({}/{}) — another runner instance may be active;"
                                        + " retrying after {}",
                                consecutiveConflicts,
                                options.maxConsecutivePollConflicts(),
                                options.pollRetryDelay());
                        sleep(options.pollRetryDelay());
                        continue;
                    }
                    // Non-409 transient — unrelated to instance ownership, so a
                    // recovered outage must not accumulate with later conflicts.
                    consecutiveConflicts = 0;
                    log.warn("Poll failed with HTTP {} — backing off {}", e.statusCode(), options.pollRetryDelay());
                    sleep(options.pollRetryDelay());
                    continue;
                } catch (Exception e) {
                    consecutiveConflicts = 0;
                    consecutiveAuthFailures = 0;
                    log.warn("Poll failed: {} — backing off {}", e.toString(), options.pollRetryDelay());
                    sleep(options.pollRetryDelay());
                    continue;
                }
                // Poll succeeded — the other instance must have died or released
                // the identity, so the conflict streak starts over. The auth budget
                // starts over with it: the credential just worked, so an earlier 401
                // must not still count against a runner that has been healthy since
                // (issue #507).
                consecutiveConflicts = 0;
                consecutiveAuthFailures = 0;
                if (response != null && response.cancel() != null) {
                    for (String id : response.cancel()) {
                        dispatcher.cancel(id);
                    }
                }
                if (atCapacity) {
                    // Work is always empty in this branch (server-side
                    // capacity check); cancels above are already processed.
                    sleep(options.capacityBackoff());
                    continue;
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
