package io.croniq.runner.internal;

import io.croniq.runner.handler.CroniqJobHandler;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;

/**
 * Maps {@code job_key} → handler. A default fallback handler can be registered
 * for ad-hoc test runners or for shell-exec style dispatch.
 *
 * <p>Each handler may optionally carry a {@code schedule} string. Scheduled
 * handlers self-register with the server via {@code POST /v1/jobs/register}
 * when the runner starts.
 */
public final class HandlerRegistry {

    private final Map<String, CroniqJobHandler> byKey;
    private final Map<String, String> schedules;
    private final CroniqJobHandler defaultHandler;

    public HandlerRegistry(
            Map<String, CroniqJobHandler> byKey, Map<String, String> schedules, CroniqJobHandler defaultHandler) {
        this.byKey = Map.copyOf(byKey);
        this.schedules = Map.copyOf(schedules);
        this.defaultHandler = defaultHandler;
    }

    public Optional<CroniqJobHandler> resolve(String jobKey) {
        CroniqJobHandler exact = byKey.get(jobKey);
        if (exact != null) {
            return Optional.of(exact);
        }
        return Optional.ofNullable(defaultHandler);
    }

    /** Snapshot of handlers that carry a schedule, in registration order. */
    public List<ScheduledHandler> scheduled() {
        List<ScheduledHandler> out = new ArrayList<>(schedules.size());
        for (var e : schedules.entrySet()) {
            out.add(new ScheduledHandler(e.getKey(), e.getValue()));
        }
        return out;
    }

    public record ScheduledHandler(String jobKey, String schedule) {}

    public static Builder builder() {
        return new Builder();
    }

    public static final class Builder {
        private final Map<String, CroniqJobHandler> byKey = new HashMap<>();
        // LinkedHashMap so registration order is preserved — the conformance
        // suite relies on this when asserting on multiple registrations.
        private final Map<String, String> schedules = new LinkedHashMap<>();
        private CroniqJobHandler defaultHandler;

        public Builder add(String jobKey, CroniqJobHandler handler) {
            return add(jobKey, null, handler);
        }

        public Builder add(String jobKey, String schedule, CroniqJobHandler handler) {
            if (byKey.putIfAbsent(jobKey, handler) != null) {
                throw new IllegalStateException("Duplicate handler registration for job_key=" + jobKey);
            }
            if (schedule != null && !schedule.isBlank()) {
                schedules.put(jobKey, schedule);
            }
            return this;
        }

        public Builder defaultHandler(CroniqJobHandler handler) {
            this.defaultHandler = handler;
            return this;
        }

        public HandlerRegistry build() {
            return new HandlerRegistry(byKey, schedules, defaultHandler);
        }
    }
}
