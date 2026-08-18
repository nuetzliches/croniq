package io.croniq.runner.internal;

import io.croniq.runner.CroniqHttpException;
import io.croniq.runner.config.CroniqRunnerOptions;
import io.croniq.runner.handler.CroniqCancellation;
import io.croniq.runner.handler.CroniqJobHandler;
import io.croniq.runner.handler.CroniqRunnerObserver;
import io.croniq.runner.protocol.AckRequest;
import io.croniq.runner.protocol.RenewRequest;
import io.croniq.runner.protocol.WorkAssignment;
import java.time.Duration;
import java.util.List;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
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
        inflight.put(work.executionId(), handle);
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
            inflight.remove(work.executionId());
            long durationMs = Duration.ofNanos(System.nanoTime() - startNanos).toMillis();
            notifyEnd(work, status, error, durationMs);
            sendAck(work, status, error, durationMs);
            MDC.remove("execution_id");
            MDC.remove("job_key");
        }
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

    private void sendAck(WorkAssignment work, String status, String error, long durationMs) {
        try {
            client.ack(new AckRequest(runnerId, work.executionId(), status, error, durationMs, work.attempt()));
        } catch (CroniqHttpException e) {
            if (e.isOwnershipDenied()) {
                // Permanent (#436/#437) — the execution stays claimed until its
                // lease expires, so name the fix rather than just the failure.
                log.error(
                        "ack refused with 403 Forbidden — this runner's credential does not own runner_id {}, so"
                                + " the execution stays claimed until its lease expires. Give the runner its own"
                                + " runner_id, or release the existing binding with DELETE /v1/runners/{id}",
                        runnerId);
            } else {
                log.warn("Failed to ack execution (status={}): HTTP {}", status, e.statusCode());
            }
        } catch (Exception e) {
            // Ack failures are logged and dropped — the server will eventually
            // re-issue the work and the next attempt will land.
            // Both callers (runOne, rejectAssignment) put execution_id in the
            // MDC before getting here, so the record is still attributable.
            log.warn("Failed to ack execution (status={}): {}", status, e.toString());
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
