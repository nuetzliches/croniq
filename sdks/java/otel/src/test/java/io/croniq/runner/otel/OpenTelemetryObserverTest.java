package io.croniq.runner.otel;

import static org.assertj.core.api.Assertions.assertThat;

import io.croniq.runner.handler.CroniqRunnerObserver;
import io.opentelemetry.api.trace.StatusCode;
import io.opentelemetry.sdk.OpenTelemetrySdk;
import io.opentelemetry.sdk.testing.exporter.InMemorySpanExporter;
import io.opentelemetry.sdk.trace.SdkTracerProvider;
import io.opentelemetry.sdk.trace.export.SimpleSpanProcessor;
import java.time.Duration;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class OpenTelemetryObserverTest {

    private InMemorySpanExporter exporter;
    private OpenTelemetrySdk otel;
    private OpenTelemetryObserver observer;

    @BeforeEach
    void setUp() {
        exporter = InMemorySpanExporter.create();
        otel = OpenTelemetrySdk.builder()
                .setTracerProvider(SdkTracerProvider.builder()
                        .addSpanProcessor(SimpleSpanProcessor.create(exporter))
                        .build())
                .build();
        observer = new OpenTelemetryObserver(otel);
    }

    @AfterEach
    void tearDown() {
        otel.close();
    }

    @Test
    void successfulExecutionEmitsSpanWithStandardAttributes() {
        observer.onExecutionStart(
                new CroniqRunnerObserver.ExecutionStart("exec-001", "billing:invoice", 1, "runner-abc"));
        observer.onExecutionEnd(new CroniqRunnerObserver.ExecutionEnd(
                "exec-001", "billing:invoice", 1, "runner-abc", "success", null, Duration.ofMillis(123)));

        var spans = exporter.getFinishedSpanItems();
        assertThat(spans).hasSize(1);
        var span = spans.get(0);
        assertThat(span.getName()).isEqualTo("croniq.execute billing:invoice");
        assertThat(span.getAttributes().get(OpenTelemetryObserver.Attributes.JOB_KEY))
                .isEqualTo("billing:invoice");
        assertThat(span.getAttributes().get(OpenTelemetryObserver.Attributes.EXECUTION_ID))
                .isEqualTo("exec-001");
        assertThat(span.getAttributes().get(OpenTelemetryObserver.Attributes.EXECUTION_ATTEMPT))
                .isEqualTo(1L);
        assertThat(span.getAttributes().get(OpenTelemetryObserver.Attributes.RUNNER_ID))
                .isEqualTo("runner-abc");
        assertThat(span.getAttributes().get(OpenTelemetryObserver.Attributes.EXECUTION_OUTCOME))
                .isEqualTo("success");
        assertThat(span.getStatus().getStatusCode()).isEqualTo(StatusCode.UNSET);
    }

    @Test
    void failedExecutionMarksSpanErrorWithMessage() {
        observer.onExecutionStart(
                new CroniqRunnerObserver.ExecutionStart("exec-fail", "billing:invoice", 2, "runner-abc"));
        observer.onExecutionEnd(new CroniqRunnerObserver.ExecutionEnd(
                "exec-fail",
                "billing:invoice",
                2,
                "runner-abc",
                "failure",
                "downstream timeout",
                Duration.ofMillis(5000)));

        var spans = exporter.getFinishedSpanItems();
        assertThat(spans).hasSize(1);
        var span = spans.get(0);
        assertThat(span.getStatus().getStatusCode()).isEqualTo(StatusCode.ERROR);
        assertThat(span.getStatus().getDescription()).isEqualTo("downstream timeout");
        assertThat(span.getAttributes().get(OpenTelemetryObserver.Attributes.EXECUTION_OUTCOME))
                .isEqualTo("failure");
    }

    @Test
    void endWithoutStartIsHarmless() {
        // Defensive: late-arriving end without a matching start (would only
        // happen if an observer was registered mid-flight, but proves the
        // ConcurrentHashMap lookup tolerates a miss).
        observer.onExecutionEnd(
                new CroniqRunnerObserver.ExecutionEnd("exec-orphan", "j", 1, "r", "success", null, Duration.ZERO));
        assertThat(exporter.getFinishedSpanItems()).isEmpty();
    }
}
