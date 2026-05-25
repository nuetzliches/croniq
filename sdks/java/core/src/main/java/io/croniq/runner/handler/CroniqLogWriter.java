package io.croniq.runner.handler;

import java.util.Map;

/**
 * Streaming, structured log sink for the duration of one execution. The writer
 * batches events on a background thread and POSTs them to
 * {@code /v1/work/{execution_id}/events}; the SDK guarantees that every
 * accepted write is flushed before the execution is acked (drain-before-ack).
 *
 * <p>Backpressure: the underlying queue is bounded. If the handler outpaces
 * the flusher, {@link #write} will block — preventing the runner from
 * growing an unbounded in-memory buffer.
 *
 * <p>Standard fields ({@code job_key}, {@code runner_id},
 * {@code runner_tags}) are injected automatically; caller-provided keys with
 * those names take precedence.
 */
public interface CroniqLogWriter {

    /** Emit one event at the given level. Common levels: trace, debug, info, warn, error. */
    void write(String level, String message);

    /** Emit one event with caller-defined structured fields. */
    void write(String level, String message, Map<String, String> fields);
}
