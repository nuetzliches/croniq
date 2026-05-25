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
 * <p>All five behaviours are wired: {@code noop}, {@code throw}, {@code sleep},
 * {@code log}, {@code stream_logs}. {@code stream_logs} emits via
 * {@link io.croniq.runner.handler.CroniqLogWriter}; the dispatcher drains the
 * writer before acking.
 */
final class HandlerSentinels {

    private HandlerSentinels() {}

    static void applyTo(CroniqRunner.Builder builder, List<CaseSpec.HandlerSpec> handlers) {
        for (CaseSpec.HandlerSpec h : handlers) {
            CroniqJobHandler handler = forBehavior(h);
            if (Boolean.TRUE.equals(h.isDefault())) {
                builder.defaultHandler(handler);
            } else if (h.schedule() != null && !h.schedule().isBlank()) {
                builder.addJob(h.jobKey(), h.schedule(), handler);
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
        return ctx -> {
            int count = h.count() == null ? 1 : h.count();
            long interval = h.intervalMs() == null ? 0L : h.intervalMs();
            String level = h.level() == null ? "info" : h.level().toLowerCase(Locale.ROOT);
            for (int i = 0; i < count; i++) {
                ctx.logWriter().write(level, "line " + i);
                if (interval > 0) {
                    Thread.sleep(interval);
                }
            }
        };
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
