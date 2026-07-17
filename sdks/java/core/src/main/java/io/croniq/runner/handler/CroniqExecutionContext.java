package io.croniq.runner.handler;

import com.fasterxml.jackson.databind.JsonNode;
import java.time.Duration;
import java.time.Instant;
import java.util.List;
import org.slf4j.Logger;

/**
 * Per-execution context handed to a {@link CroniqJobHandler}. The reference is
 * stable for the lifetime of the execution; handler implementations may freely
 * pass it to inner helpers.
 *
 * <p>The {@link #metadata()} {@link JsonNode} is the opaque payload the server
 * delivered with the assignment — its shape is job-specific and the SDK does
 * not interpret it.
 */
public interface CroniqExecutionContext {

    /** Server-assigned unique id for this execution. */
    String executionId();

    /** Job key (e.g., {@code "billing:invoice"}). */
    String jobKey();

    /**
     * The trigger's original logical fire time — stable across retries and
     * dead-letter replays. Use this (not {@link Instant#now()}) for
     * time-relative job logic like "the month being reported". {@code null}
     * when the server predates the field; the SDK never falls back to the
     * queue fire time.
     */
    Instant scheduledFor();

    /** 1-based attempt counter. */
    int attempt();

    /** Opaque metadata payload from the server; may be {@link JsonNode#isNull()}. */
    JsonNode metadata();

    /** Parsed handler-side timeout. The SDK does not enforce this — handlers may use it as a hint. */
    Duration timeout();

    /** Stable runner identifier — useful for correlating logs across the cluster. */
    String runnerId();

    /** Runner-level tags (e.g., {@code "lang=java"}, {@code "env=prod"}). */
    List<String> runnerTags();

    /** SLF4J logger pre-bound with {@code execution_id} / {@code job_key} MDC entries. */
    Logger logger();

    /** Cancellation handle — fires when the server requests cancellation or the runner is draining. */
    CroniqCancellation cancellation();

    /**
     * Streaming structured-log writer. Every event written here is shipped to
     * the server via {@code POST /v1/work/{execution_id}/events}; the dispatcher
     * drains and closes the writer before sending the final ack, so log lines
     * for an execution always land before the execution is marked complete.
     */
    CroniqLogWriter logWriter();
}
