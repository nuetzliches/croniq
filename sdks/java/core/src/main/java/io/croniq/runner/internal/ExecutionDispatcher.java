package io.croniq.runner.internal;

import io.croniq.runner.config.CroniqRunnerOptions;
import io.croniq.runner.handler.CroniqCancellation;
import io.croniq.runner.handler.CroniqJobHandler;
import io.croniq.runner.protocol.AckRequest;
import io.croniq.runner.protocol.RenewRequest;
import io.croniq.runner.protocol.WorkAssignment;
import java.time.Duration;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

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
    private final ConcurrentHashMap<String, CancellationHandle> inflight = new ConcurrentHashMap<>();

    public ExecutionDispatcher(
            CroniqClient client,
            HandlerRegistry registry,
            ExecutorService executor,
            CroniqRunnerOptions options,
            String runnerId) {
        this.client = client;
        this.registry = registry;
        this.executor = executor;
        this.options = options;
        this.runnerId = runnerId;
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
     */
    public void dispatch(WorkAssignment work) {
        CancellationHandle handle = new CancellationHandle();
        inflight.put(work.executionId(), handle);
        executor.execute(() -> runOne(work, handle));
    }

    private void runOne(WorkAssignment work, CancellationHandle handle) {
        long startNanos = System.nanoTime();
        handle.attach(Thread.currentThread());
        // Renewal loop runs alongside the handler. We interrupt it after the
        // handler finishes; the virtual thread exits on its own and the JVM
        // doesn't care about lingering ones.
        Thread renewer = startRenewalLoop(work.executionId(), handle);
        String status = AckRequest.Status.SUCCESS;
        String error = null;
        try {
            CroniqJobHandler handler = registry.resolve(work.jobKey())
                    .orElseThrow(() -> new IllegalStateException("No handler registered for job_key=" + work.jobKey()));
            Duration timeout = parseTimeoutOrZero(work.timeout());
            var ctx = new ExecutionContextImpl(
                    work.executionId(),
                    work.jobKey(),
                    work.attempt(),
                    work.metadata(),
                    timeout,
                    runnerId,
                    options.tags(),
                    handle);
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
                log.debug("Handler for {} threw", work.executionId(), e);
            }
        } catch (RuntimeException e) {
            status = AckRequest.Status.FAILURE;
            error = e.getMessage();
            log.warn("Dispatcher failure for execution {}", work.executionId(), e);
        } finally {
            renewer.interrupt();
            inflight.remove(work.executionId());
            long durationMs = Duration.ofNanos(System.nanoTime() - startNanos).toMillis();
            sendAck(work, status, error, durationMs);
        }
    }

    private Thread startRenewalLoop(String executionId, CancellationHandle handle) {
        return Thread.ofVirtual().name("croniq-renew-" + executionId).start(() -> {
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
                } catch (Exception e) {
                    // Transient failures are logged and ignored — the server
                    // will mark the execution as stalled if no heartbeats
                    // arrive. Hard-failing here would conflict with the
                    // handler's own error reporting.
                    log.debug("Renew failed for {}: {}", executionId, e.toString());
                }
            }
        });
    }

    private void sendAck(WorkAssignment work, String status, String error, long durationMs) {
        try {
            client.ack(new AckRequest(runnerId, work.executionId(), status, error, durationMs, work.attempt()));
        } catch (Exception e) {
            // Ack failures are logged and dropped — the server will eventually
            // re-issue the work and the next attempt will land.
            log.warn("Failed to ack execution {} (status={}): {}", work.executionId(), status, e.toString());
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
}
