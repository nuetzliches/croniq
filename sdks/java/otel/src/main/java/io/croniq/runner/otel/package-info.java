/**
 * OpenTelemetry instrumentation for the Croniq Java SDK. Register
 * {@link io.croniq.runner.otel.OpenTelemetryObserver} via
 * {@code CroniqRunner.Builder.observer(...)} to emit a span per execution.
 *
 * <p>Span name: {@code croniq.execute &lt;job_key&gt;}.
 *
 * <p>Standard attributes (mirrors the .NET SDK):
 *
 * <ul>
 *   <li>{@code croniq.job.key}
 *   <li>{@code croniq.execution.id}
 *   <li>{@code croniq.execution.attempt}
 *   <li>{@code croniq.runner.id}
 *   <li>{@code croniq.execution.outcome} (success / failure)
 * </ul>
 */
package io.croniq.runner.otel;
