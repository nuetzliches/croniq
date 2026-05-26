package io.croniq.runner.handler;

import java.time.Duration;

/**
 * Lifecycle hook for downstream observability (tracing, metrics, audit). The
 * core SDK calls these methods around every execution; without an observer
 * registered they are inert. The companion
 * {@code io.croniq:runner-opentelemetry} module ships an observer that emits
 * an OpenTelemetry span per execution.
 *
 * <p>Implementations must be non-blocking and exception-safe — any throwable
 * the observer raises is logged and swallowed so observability never breaks
 * job dispatch.
 */
public interface CroniqRunnerObserver {

    /** Called immediately before the handler is invoked. */
    default void onExecutionStart(ExecutionStart event) {}

    /** Called after the handler returns / throws and before the ack is sent. */
    default void onExecutionEnd(ExecutionEnd event) {}

    /** Snapshot of execution-start context. */
    record ExecutionStart(String executionId, String jobKey, int attempt, String runnerId) {}

    /** Snapshot of execution-end context. {@code error} is {@code null} on success. */
    record ExecutionEnd(
            String executionId,
            String jobKey,
            int attempt,
            String runnerId,
            String status,
            String error,
            Duration duration) {}
}
