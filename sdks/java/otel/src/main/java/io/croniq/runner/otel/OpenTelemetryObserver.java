package io.croniq.runner.otel;

import io.croniq.runner.handler.CroniqRunnerObserver;
import io.opentelemetry.api.OpenTelemetry;
import io.opentelemetry.api.common.AttributeKey;
import io.opentelemetry.api.trace.Span;
import io.opentelemetry.api.trace.SpanKind;
import io.opentelemetry.api.trace.StatusCode;
import io.opentelemetry.api.trace.Tracer;
import java.util.concurrent.ConcurrentHashMap;

/**
 * {@link CroniqRunnerObserver} that emits an OpenTelemetry span per execution.
 *
 * <p>Wire in via the SDK builder:
 *
 * <pre>{@code
 * CroniqRunner runner = CroniqRunner.builder()
 *         .options(options)
 *         .observer(new OpenTelemetryObserver(OpenTelemetry.getGlobalOpenTelemetry()))
 *         .build();
 * }</pre>
 *
 * <p>Span attributes follow the conventions used by the .NET SDK so traces
 * are uniform across runtime languages. See {@link Attributes} for the
 * canonical attribute keys.
 */
public final class OpenTelemetryObserver implements CroniqRunnerObserver {

    /**
     * Standard span attribute keys. Stable string constants — exported so users
     * can correlate Croniq spans with their own instrumentation without
     * hard-coding the strings.
     */
    public static final class Attributes {
        public static final AttributeKey<String> JOB_KEY = AttributeKey.stringKey("croniq.job.key");
        public static final AttributeKey<String> EXECUTION_ID = AttributeKey.stringKey("croniq.execution.id");
        public static final AttributeKey<Long> EXECUTION_ATTEMPT = AttributeKey.longKey("croniq.execution.attempt");
        public static final AttributeKey<String> RUNNER_ID = AttributeKey.stringKey("croniq.runner.id");
        public static final AttributeKey<String> EXECUTION_OUTCOME = AttributeKey.stringKey("croniq.execution.outcome");

        private Attributes() {}
    }

    private static final String INSTRUMENTATION_NAME = "io.croniq.runner";

    private final Tracer tracer;
    // Spans are stored by execution_id rather than ThreadLocal because the
    // start and end callbacks may run on different virtual threads (the
    // dispatcher invokes notifyStart() on the handler's worker thread and
    // notifyEnd() in the finally block of the same thread, but storing in
    // a map is robust to future changes — and lookup is O(1) anyway).
    private final ConcurrentHashMap<String, Span> activeSpans = new ConcurrentHashMap<>();

    public OpenTelemetryObserver(OpenTelemetry openTelemetry) {
        this(openTelemetry.getTracer(
                INSTRUMENTATION_NAME, OpenTelemetryObserver.class.getPackage().getImplementationVersion()));
    }

    /** Constructor for callers who pre-build their own {@link Tracer}. */
    public OpenTelemetryObserver(Tracer tracer) {
        this.tracer = tracer;
    }

    @Override
    public void onExecutionStart(ExecutionStart event) {
        Span span = tracer.spanBuilder("croniq.execute " + event.jobKey())
                .setSpanKind(SpanKind.CONSUMER)
                .setAttribute(Attributes.JOB_KEY, event.jobKey())
                .setAttribute(Attributes.EXECUTION_ID, event.executionId())
                .setAttribute(Attributes.EXECUTION_ATTEMPT, (long) event.attempt())
                .setAttribute(Attributes.RUNNER_ID, event.runnerId())
                .startSpan();
        activeSpans.put(event.executionId(), span);
    }

    @Override
    public void onExecutionEnd(ExecutionEnd event) {
        Span span = activeSpans.remove(event.executionId());
        if (span == null) {
            return;
        }
        span.setAttribute(Attributes.EXECUTION_OUTCOME, event.status());
        if (event.error() != null && !event.error().isBlank()) {
            span.setStatus(StatusCode.ERROR, event.error());
        }
        span.end();
    }
}
