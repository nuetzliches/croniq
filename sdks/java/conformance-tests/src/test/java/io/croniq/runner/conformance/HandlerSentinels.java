package io.croniq.runner.conformance;

import io.croniq.runner.CroniqRunner;
import io.croniq.runner.handler.CroniqExecutionContext;
import io.croniq.runner.handler.CroniqJobHandler;
import java.util.List;
import java.util.Locale;

/**
 * Translates the YAML {@code handlers} block into real
 * {@link CroniqJobHandler} registrations on the
 * {@link io.croniq.runner.CroniqRunner.Builder}.
 *
 * <p>PR-2 implements the {@code noop}, {@code throw}, and {@code sleep}
 * behaviours — enough to make cases 01-04 pass. {@code log} / {@code stream_logs}
 * land in PR-4 with the streaming log writer.
 */
final class HandlerSentinels {

    private HandlerSentinels() {}

    static void applyTo(CroniqRunner.Builder builder, List<CaseSpec.HandlerSpec> handlers) {
        for (CaseSpec.HandlerSpec h : handlers) {
            CroniqJobHandler handler = forBehavior(h);
            if (Boolean.TRUE.equals(h.isDefault())) {
                builder.defaultHandler(handler);
            } else {
                builder.addJob(h.jobKey(), handler);
            }
        }
    }

    private static CroniqJobHandler forBehavior(CaseSpec.HandlerSpec h) {
        return switch (h.behavior()) {
            case "noop" -> ctx -> {};
            case "throw" -> ctx -> {
                throw new RuntimeException(h.errorMessage() != null ? h.errorMessage() : "handler threw");
            };
            case "sleep" -> ctx -> {
                long ms = h.durationMs() == null ? 0 : h.durationMs();
                Thread.sleep(ms);
            };
            case "log" -> logSentinel(h);
            case "stream_logs" -> streamLogsSentinel(h);
            default -> throw new IllegalArgumentException("Unknown handler behaviour: " + h.behavior());
        };
    }

    private static CroniqJobHandler logSentinel(CaseSpec.HandlerSpec h) {
        // PR-2 emits via the SLF4J logger only — that's enough to verify
        // the handler ran. PR-4 wires the WorkEvent streaming writer.
        return ctx -> {
            int count = h.count() == null ? 1 : h.count();
            String level = h.level() == null ? "info" : h.level().toLowerCase(Locale.ROOT);
            for (int i = 0; i < count; i++) {
                logAtLevel(ctx, level, h.message());
            }
        };
    }

    private static CroniqJobHandler streamLogsSentinel(CaseSpec.HandlerSpec h) {
        // PR-4 wires the LogWriter; until then this is a no-op so cases that
        // *only* depend on stream_logs being installable don't block PR-2.
        return ctx -> {};
    }

    private static void logAtLevel(CroniqExecutionContext ctx, String level, String msg) {
        switch (level) {
            case "trace" -> ctx.logger().trace(msg);
            case "debug" -> ctx.logger().debug(msg);
            case "warn" -> ctx.logger().warn(msg);
            case "error" -> ctx.logger().error(msg);
            default -> ctx.logger().info(msg);
        }
    }
}
