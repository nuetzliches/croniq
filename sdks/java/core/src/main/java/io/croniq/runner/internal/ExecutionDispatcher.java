package io.croniq.runner.internal;

import io.croniq.runner.CroniqHttpException;
import io.croniq.runner.config.CroniqRunnerOptions;
import io.croniq.runner.handler.CroniqCancellation;
import io.croniq.runner.handler.CroniqJobHandler;
import io.croniq.runner.handler.CroniqRunnerObserver;
import io.croniq.runner.protocol.AckRequest;
import io.croniq.runner.protocol.RenewRequest;
import io.croniq.runner.protocol.WorkAssignment;
import java.io.IOException;
import java.time.Duration;
import java.util.List;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Semaphore;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.slf4j.MDC;

/**
 * Owns the in-flight execution table and the per-handler lifecycle:
 * build context, invoke handler on a virtual thread, ack the result, drop
 * the in-flight slot. PR-3 layers on the renewal loop and drain support;
 * PR-4 layers on the streaming log writer.
 */
public final class ExecutionDispatcher {

    private static final Logger log = LoggerFactory.getLogger(ExecutionDispatcher.class);

    private final CroniqClient client;
    private final HandlerRegistry registry;
    private final ExecutorService executor;
    private final CroniqRunnerOptions options;
    private final String runnerId;
    private final List<CroniqRunnerObserver> observers;
    private final ConcurrentHashMap<String, CancellationHandle> inflight = new ConcurrentHashMap<>();
    // One permit per released in-flight slot; lets the poll loop end its
    // at-capacity backoff as soon as there is room (issue #845).
    private final Semaphore slotFreed = new Semaphore(0);
    private final AtomicInteger acking = new AtomicInteger();

    /**
     * Delays before the second and third ack attempt. Package-private and
     * mutable only so unit tests can shorten them; see {@link #sendAck}.
     */
    static final List<Duration> DEFAULT_ACK_RETRY_DELAYS = List.of(Duration.ofSeconds(1), Duration.ofSeconds(2));

    volatile List<Duration> ackRetryDelays = DEFAULT_ACK_RETRY_DELAYS;

    public ExecutionDispatcher(
            CroniqClient client,
            HandlerRegistry registry,
            ExecutorService executor,
            CroniqRunnerOptions options,
            String runnerId,
            List<CroniqRunnerObserver> observers) {
        this.client = client;
        this.registry = registry;
        this.executor = executor;
        this.options = options;
        this.runnerId = runnerId;
        this.observers = List.copyOf(observers);
    }

    public int inflightCount() {
        return inflight.size();
    }

    public java.util.Set<String> inflightIds() {
        return java.util.Set.copyOf(inflight.keySet());
    }

    /**
     * Forget slots freed so far. Called before each in-flight snapshot: a
     * handler that finishes after it leaves a fresh signal, and one that
     * finished before it is already missing from the snapshot.
     */
    public void resetSlotFreed() {
        slotFreed.drainPermits();
    }

    /**
     * Wait up to {@code timeout} for an in-flight slot to be released since the
     * last {@link #resetSlotFreed()}. Returns early the moment one is.
     */
    public void awaitSlotFreed(Duration timeout) throws InterruptedException {
        long ms = timeout == null ? 0 : Math.max(0, timeout.toMillis());
        if (ms > 0) {
            slotFreed.tryAcquire(ms, TimeUnit.MILLISECONDS);
        }
    }

    /** Cancel an in-flight execution by id. No-op if the id is unknown. */
    public void cancel(String executionId) {
        CancellationHandle h = inflight.get(executionId);
        if (h != null) {
            h.cancel();
        }
    }

    /**
     * Submit a work assignment for handling. Returns immediately; the actual
     * handler runs on a virtual thread inside the executor.
     *
     * <p>An assignment carrying a control character in either identifier is
     * refused here, before it can reach a handler, a log record or a telemetry
     * attribute. See {@link IdentifierGuard} for the rule and why it is a
     * denylist.
     */
    public void dispatch(WorkAssignment work) {
        String rejected = IdentifierGuard.rejectAssignmentReason(work.executionId(), work.jobKey());
        if (rejected != null) {
            rejectAssignment(work, rejected);
            return;
        }
        CancellationHandle handle = new CancellationHandle();
        // A correct server never re-delivers a claimed execution, but a server
        // bug or a replayed response can. Dispatching it again would run the
        // handler twice and overwrite the first run's cancel handle, so a later
        // server cancel would no longer reach it (review 2026-10-24, K-11).
        if (inflight.putIfAbsent(work.executionId(), handle) != null) {
            log.warn("Ignoring re-delivered execution that is still in flight");
            return;
        }
        executor.execute(() -> runOne(work, handle));
    }

    /**
     * Handle a work assignment refused by the ingest guard.
     *
     * <p>The two cases differ in what the runner can still tell the server:
     *
     * <ul>
     *   <li><b>Unsafe {@code execution_id}</b> — nothing. That value is what
     *       addresses an ack or renew, so there is no way to report anything
     *       about this execution. The assignment is dropped and the server's
     *       lease expires.
     *   <li><b>Unsafe {@code job_key}, valid {@code execution_id}</b> — a
     *       failure ack. The handler never runs, but the execution completes
     *       with an error naming the offending field, so the operator sees a
     *       dead-lettered execution instead of one that is silently requeued by
     *       the stale-claim reaper and refused again on every later poll.
     * </ul>
     *
     * <p>Runs inline on the poll thread rather than on the handler executor:
     * this path only triggers on malformed input, so pausing the loop for one
     * small POST costs nothing and keeps the ordering observable.
     */
    private void rejectAssignment(WorkAssignment work, String field) {
        boolean ackable = !"execution_id".equals(field);
        String offending = ackable ? work.jobKey() : work.executionId();
        // The value is escaped and truncated: this is the one place a refused
        // value is rendered, and it is hostile by definition.
        log.warn(
                "Rejected work assignment with unsafe identifier {} (acked={}): {}",
                field,
                ackable,
                IdentifierGuard.previewForLog(offending));
        if (!ackable) {
            return;
        }
        // The execution_id is the safe half here, so it can carry the ack's
        // diagnostics as an MDC entry the way runOne does.
        MDC.put("execution_id", work.executionId());
        try {
            sendAck(work, AckRequest.Status.FAILURE, IdentifierGuard.rejectionAckError(field, offending), 0);
        } finally {
            MDC.remove("execution_id");
        }
    }

    private void runOne(WorkAssignment work, CancellationHandle handle) {
        long startNanos = System.nanoTime();
        handle.attach(Thread.currentThread());
        // Identifiers travel as MDC entries, never interpolated into a message —
        // a job_key carrying CRLF would otherwise forge a log record and one
        // carrying ANSI escapes would reach the operator's terminal raw. The
        // logging backend owns rendering, exactly as it does for every other MDC
        // entry; the values are already known safe because dispatch() validated
        // them. Cleared in the finally below.
        MDC.put("execution_id", work.executionId());
        MDC.put("job_key", work.jobKey());
        // Renewal loop runs alongside the handler. We interrupt it after the
        // handler finishes; the virtual thread exits on its own and the JVM
        // doesn't care about lingering ones.
        Thread renewer = startRenewalLoop(work.executionId(), handle);
        BoundedLogWriter logWriter = new BoundedLogWriter(client, work.executionId(), work.jobKey(), runnerId, options);
        notifyStart(work);
        String status = AckRequest.Status.SUCCESS;
        String error = null;
        try {
            CroniqJobHandler handler = registry.resolve(work.jobKey())
                    .orElseThrow(() -> new IllegalStateException("No handler registered for job_key=" + work.jobKey()));
            Duration timeout = parseTimeoutOrZero(work.timeout());
            var ctx = new ExecutionContextImpl(
                    work.executionId(),
                    work.jobKey(),
                    parseScheduledFor(work.scheduledFor()),
                    work.attempt(),
                    work.metadata(),
                    timeout,
                    runnerId,
                    options.tags(),
                    handle,
                    logWriter);
            try {
                handler.handle(ctx);
                // If the cancel arrived during a non-blocking handler the
                // interrupt won't have fired — treat a cancelled-but-clean
                // return as a failure to stay consistent with the .NET SDK.
                if (handle.isRequested()) {
                    status = AckRequest.Status.FAILURE;
                    error = "cancelled";
                }
            } catch (CroniqCancellation.CancellationException e) {
                status = AckRequest.Status.FAILURE;
                error = e.getMessage();
            } catch (InterruptedException e) {
                // Cancel arrived via Thread.interrupt() — Thread.sleep, blocking
                // I/O, etc. unwind here. Deliberately do NOT re-set the interrupt
                // flag: the ack call below uses HttpClient.send which also
                // observes interrupts, and getting the ack out is the whole
                // point of cancellation reporting.
                status = AckRequest.Status.FAILURE;
                error = "cancelled";
            } catch (Exception e) {
                status = AckRequest.Status.FAILURE;
                error = e.getMessage() != null ? e.getMessage() : e.getClass().getSimpleName();
                log.debug("Job handler threw", e);
            }
        } catch (RuntimeException e) {
            status = AckRequest.Status.FAILURE;
            error = e.getMessage();
            log.warn("Dispatcher failure", e);
        } finally {
            renewer.interrupt();
            // Drain log events BEFORE acking. This is the central
            // streaming-log guarantee — late events would otherwise arrive
            // on the server after the execution is already marked complete.
            logWriter.closeAndDrain();
            long durationMs = Duration.ofNanos(System.nanoTime() - startNanos).toMillis();
            notifyEnd(work, status, error, durationMs);
            // Ack first, free the slot after (review 2026-10-24, K-5). Freeing
            // it first let the poll loop report `inflight: []` and claim new
            // work while the ack was still on the wire, briefly running over
            // maxInflight(), and is the order no other SDK uses.
            //
            // The handler is done, so a cancel from here on has nothing left to
            // stop: detach the worker so a server cancel or the drain's
            // hard-cancel cannot interrupt the ack, and consume an interrupt that
            // already landed between the handler returning and this point.
            // HttpClient.send throws on a set interrupt flag, which lost the ack.
            handle.detach();
            Thread.interrupted();
            acking.incrementAndGet();
            try {
                sendAck(work, status, error, durationMs);
            } finally {
                acking.decrementAndGet();
                inflight.remove(work.executionId());
                slotFreed.release();
                MDC.remove("execution_id");
                MDC.remove("job_key");
            }
        }
    }

    /** Number of executions whose handler has returned and whose ack is still being sent. */
    public int ackingCount() {
        return acking.get();
    }

    /**
     * Upper bound on how long one ack can take including its retries: every
     * attempt may run into the request timeout, plus the delays between them.
     * {@code CroniqRunner.close()} waits this long for acks still in progress
     * after the drain timeout.
     */
    public Duration maxAckDuration() {
        Duration total = options.requestTimeout().multipliedBy(ackRetryDelays.size() + 1L);
        for (Duration d : ackRetryDelays) {
            total = total.plus(d);
        }
        return total;
    }

    private void notifyStart(WorkAssignment work) {
        if (observers.isEmpty()) {
            return;
        }
        var event =
                new CroniqRunnerObserver.ExecutionStart(work.executionId(), work.jobKey(), work.attempt(), runnerId);
        for (var obs : observers) {
            try {
                obs.onExecutionStart(event);
            } catch (RuntimeException e) {
                log.debug("Observer onExecutionStart threw — swallowing", e);
            }
        }
    }

    private void notifyEnd(WorkAssignment work, String status, String error, long durationMs) {
        if (observers.isEmpty()) {
            return;
        }
        var event = new CroniqRunnerObserver.ExecutionEnd(
                work.executionId(),
                work.jobKey(),
                work.attempt(),
                runnerId,
                status,
                error,
                Duration.ofMillis(durationMs));
        for (var obs : observers) {
            try {
                obs.onExecutionEnd(event);
            } catch (RuntimeException e) {
                log.debug("Observer onExecutionEnd threw — swallowing", e);
            }
        }
    }

    private Thread startRenewalLoop(String executionId, CancellationHandle handle) {
        return Thread.ofVirtual().name("croniq-renew-" + executionId).start(() -> {
            // Own thread, so it needs its own MDC scope — the dispatcher's does
            // not propagate here.
            MDC.put("execution_id", executionId);
            long intervalMs = Math.max(50, options.renewInterval().toMillis());
            while (!handle.isRequested() && !Thread.currentThread().isInterrupted()) {
                try {
                    Thread.sleep(intervalMs);
                } catch (InterruptedException e) {
                    return; // handler finished — renewer exits cleanly
                }
                if (handle.isRequested() || Thread.currentThread().isInterrupted()) {
                    return;
                }
                try {
                    client.renew(new RenewRequest(runnerId, executionId));
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    return;
                } catch (CroniqHttpException e) {
                    if (e.isOwnershipDenied()) {
                        // Permanent (#436/#437): every later renew fails the
                        // same way and the lease will expire mid-handler.
                        log.error(
                                "lease renew refused with 403 Forbidden — this runner's credential does not own"
                                        + " runner_id {}, so the lease will expire and the execution be reclaimed."
                                        + " Give the runner its own runner_id, or release the existing binding with"
                                        + " DELETE /v1/runners/{id}",
                                runnerId);
                    } else {
                        // Since #447 renew is a real per-execution lease: 404
                        // (no longer leased here) and 409 (already terminal)
                        // are the normal outcome of a renew racing our own
                        // completion, so they stay at debug alongside the
                        // transient failures.
                        log.debug("Renew failed with HTTP {}", e.statusCode());
                    }
                } catch (Exception e) {
                    // Transient failures are logged and ignored — the server
                    // will mark the execution as stalled if no heartbeats
                    // arrive. Hard-failing here would conflict with the
                    // handler's own error reporting.
                    log.debug("Renew failed: {}", e.toString());
                }
            }
        });
    }

    /**
     * POST the ack, retrying a transient failure (review 2026-10-24, K-1).
     *
     * <p>Without a retry, one connection reset or request timeout on the ack of
     * a handler that had already succeeded cost a full second run: the
     * execution left the in-flight set, the next poll reported it missing, and
     * the stale-claim reaper requeued it. The ack is idempotent on the server
     * (CAS-fenced), so sending it again is safe.
     *
     * <p>Up to three attempts, about 1 s and then 2 s apart, the same policy in
     * every SDK and pinned by conformance cases 23–25. Only a transport error
     * (an {@link IOException} that is not an HTTP status, including
     * {@link java.net.http.HttpTimeoutException}) or a 5xx is retried. A 4xx is
     * the server's verdict on this ack (409: already terminal or no longer ours,
     * 404: gone) and sending it again cannot change it. The caller keeps the
     * execution in flight until this returns.
     *
     * <p>Both callers (runOne, rejectAssignment) put execution_id in the MDC
     * before getting here, so every record is attributable.
     */
    private void sendAck(WorkAssignment work, String status, String error, long durationMs) {
        AckRequest request = new AckRequest(runnerId, work.executionId(), status, error, durationMs, work.attempt());
        List<Duration> delays = ackRetryDelays;
        int maxAttempts = delays.size() + 1;
        for (int attempt = 1; ; attempt++) {
            String failure;
            try {
                client.ack(request);
                return;
            } catch (CroniqHttpException e) {
                if (e.isOwnershipDenied()) {
                    // Permanent (#436/#437) — the execution stays claimed until its
                    // lease expires, so name the fix rather than just the failure.
                    log.error(
                            "ack refused with 403 Forbidden — this runner's credential does not own runner_id {}, so"
                                    + " the execution stays claimed until its lease expires. Give the runner its own"
                                    + " runner_id, or release the existing binding with DELETE /v1/runners/{id}",
                            runnerId);
                    return;
                }
                if (e.statusCode() < 500) {
                    log.warn("Failed to ack execution (status={}): HTTP {} — not retried", status, e.statusCode());
                    return;
                }
                failure = "HTTP " + e.statusCode();
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                log.error("Ack of execution (status={}) interrupted — giving up", status);
                return;
            } catch (IOException e) {
                failure = e.toString();
            } catch (RuntimeException e) {
                log.error("Failed to ack execution (status={}): {}", status, e.toString());
                return;
            }
            if (attempt >= maxAttempts) {
                log.error(
                        "Failed to ack execution (status={}) after {} attempts: {} — the server will requeue it once"
                                + " the claim goes stale",
                        status,
                        attempt,
                        failure);
                return;
            }
            Duration delay = delays.get(attempt - 1);
            log.warn(
                    "Ack attempt {}/{} failed (status={}): {} — retrying in {} ms",
                    attempt,
                    maxAttempts,
                    status,
                    failure,
                    delay.toMillis());
            try {
                Thread.sleep(delay.toMillis());
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                log.error("Ack retry of execution (status={}) interrupted — giving up", status);
                return;
            }
        }
    }

    private static Duration parseTimeoutOrZero(String value) {
        if (value == null || value.isBlank()) {
            return Duration.ZERO;
        }
        try {
            return HumanDuration.parse(value);
        } catch (IllegalArgumentException e) {
            return Duration.ZERO;
        }
    }

    /**
     * Parse the server's {@code scheduled_for} (RFC 3339) into an Instant.
     * Returns {@code null} when the field is absent (older server) or
     * unparseable — never falls back to fire_at, which would reintroduce the
     * wrong-logical-time bug.
     */
    static java.time.Instant parseScheduledFor(String value) {
        if (value == null || value.isBlank()) {
            return null;
        }
        try {
            return java.time.Instant.parse(value);
        } catch (java.time.format.DateTimeParseException e) {
            return null;
        }
    }
}
