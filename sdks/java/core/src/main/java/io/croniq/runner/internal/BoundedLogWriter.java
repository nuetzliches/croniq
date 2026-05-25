package io.croniq.runner.internal;

import io.croniq.runner.config.CroniqRunnerOptions;
import io.croniq.runner.handler.CroniqLogWriter;
import io.croniq.runner.protocol.WorkEvent;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Bounded queue + virtual-thread flusher. Drains events to the server in
 * batches, triggered by:
 *
 * <ul>
 *   <li>Size threshold — once the queue holds {@code batchSize} events.
 *   <li>Time threshold — every {@code flushInterval} a partial batch is
 *       flushed so slow-emitters don't hold logs hostage until the run ends.
 *   <li>{@link #closeAndDrain()} — synchronous drain before the dispatcher
 *       sends the ack.
 * </ul>
 *
 * <p>Standard fields ({@code job_key}, {@code runner_id}, {@code runner_tags})
 * are injected into every event; caller-supplied values for those keys win.
 */
public final class BoundedLogWriter implements CroniqLogWriter {

    private static final Logger log = LoggerFactory.getLogger(BoundedLogWriter.class);

    /**
     * Soft cap on the in-memory queue. Drops the OLDEST event when full —
     * the alternative (block the handler) caused deadlocks in early prototypes
     * when the flusher was waiting on the server and the handler held the
     * monitor that the flusher needed.
     */
    private static final int QUEUE_CAPACITY = 10_000;

    /** Batch size — events per POST. Mirrors the .NET SDK default. */
    private static final int BATCH_SIZE = 32;

    private final LinkedBlockingQueue<WorkEvent> queue = new LinkedBlockingQueue<>(QUEUE_CAPACITY);
    private final CroniqClient client;
    private final String executionId;
    private final String jobKey;
    private final String runnerId;
    private final List<String> runnerTags;
    private final long flushIntervalMs;
    private final AtomicBoolean closed = new AtomicBoolean(false);
    private final Thread flusher;

    public BoundedLogWriter(
            CroniqClient client, String executionId, String jobKey, String runnerId, CroniqRunnerOptions options) {
        this.client = client;
        this.executionId = executionId;
        this.jobKey = jobKey;
        this.runnerId = runnerId;
        this.runnerTags = options.tags();
        // Reuse renew interval as a sensible default for log flush — both
        // are "background heartbeat" cadences. Independent tuning lands
        // when a real customer asks for it.
        this.flushIntervalMs = Math.max(50, options.renewInterval().toMillis());
        this.flusher =
                Thread.ofVirtual().name("croniq-log-flusher-" + executionId).start(this::drainLoop);
    }

    @Override
    public void write(String level, String message) {
        write(level, message, null);
    }

    @Override
    public void write(String level, String message, Map<String, String> fields) {
        if (closed.get()) {
            // Late writes after closeAndDrain() are dropped silently — the
            // handler shouldn't see exceptions during the ack handshake.
            return;
        }
        queue.offer(new WorkEvent(level, message, enrichFields(fields)));
    }

    /**
     * Synchronously flushes outstanding events and stops the flusher. Called
     * by the dispatcher before sending the ack; idempotent.
     */
    public void closeAndDrain() {
        if (!closed.compareAndSet(false, true)) {
            return;
        }
        flusher.interrupt();
        try {
            flusher.join(Math.max(2000, flushIntervalMs * 4));
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
        // Flusher exited via interrupt mid-poll: send anything still in the
        // queue synchronously, on the dispatcher's thread.
        List<WorkEvent> remaining = drainNow();
        if (!remaining.isEmpty()) {
            try {
                client.pushEvents(executionId, remaining);
            } catch (Exception e) {
                log.debug("Final log drain failed for {}: {}", executionId, e.toString());
            }
        }
    }

    private void drainLoop() {
        while (!closed.get()) {
            try {
                WorkEvent first = queue.poll(flushIntervalMs, TimeUnit.MILLISECONDS);
                List<WorkEvent> batch = new ArrayList<>(BATCH_SIZE);
                if (first != null) {
                    batch.add(first);
                    queue.drainTo(batch, BATCH_SIZE - 1);
                }
                if (batch.isEmpty()) {
                    continue;
                }
                client.pushEvents(executionId, batch);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                return;
            } catch (Exception e) {
                log.debug("Log batch push failed for {}: {}", executionId, e.toString());
                // Drop the batch we tried to send — retrying indefinitely
                // would compound backpressure into the handler. The .NET SDK
                // makes the same trade-off.
            }
        }
    }

    private List<WorkEvent> drainNow() {
        List<WorkEvent> out = new ArrayList<>(queue.size());
        queue.drainTo(out);
        return out;
    }

    private Map<String, String> enrichFields(Map<String, String> callerFields) {
        Map<String, String> out = new LinkedHashMap<>();
        out.put("job_key", jobKey);
        out.put("runner_id", runnerId);
        if (!runnerTags.isEmpty()) {
            out.put("runner_tags", String.join(",", runnerTags));
        }
        if (callerFields != null) {
            // Caller values win — they're job-specific and more informative
            // than the SDK's stock injections.
            out.putAll(callerFields);
        }
        return out;
    }
}
