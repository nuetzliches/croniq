package io.croniq.runner.internal;

import io.croniq.runner.handler.CroniqJobHandler;
import java.util.HashMap;
import java.util.Map;
import java.util.Optional;

/**
 * Maps {@code job_key} → handler. A default fallback handler can be registered
 * for ad-hoc test runners or for shell-exec style dispatch (PR-4+).
 */
public final class HandlerRegistry {

    private final Map<String, CroniqJobHandler> byKey;
    private final CroniqJobHandler defaultHandler;

    public HandlerRegistry(Map<String, CroniqJobHandler> byKey, CroniqJobHandler defaultHandler) {
        this.byKey = Map.copyOf(byKey);
        this.defaultHandler = defaultHandler;
    }

    public Optional<CroniqJobHandler> resolve(String jobKey) {
        CroniqJobHandler exact = byKey.get(jobKey);
        if (exact != null) {
            return Optional.of(exact);
        }
        return Optional.ofNullable(defaultHandler);
    }

    public static Builder builder() {
        return new Builder();
    }

    public static final class Builder {
        private final Map<String, CroniqJobHandler> byKey = new HashMap<>();
        private CroniqJobHandler defaultHandler;

        public Builder add(String jobKey, CroniqJobHandler handler) {
            if (byKey.putIfAbsent(jobKey, handler) != null) {
                throw new IllegalStateException("Duplicate handler registration for job_key=" + jobKey);
            }
            return this;
        }

        public Builder defaultHandler(CroniqJobHandler handler) {
            this.defaultHandler = handler;
            return this;
        }

        public HandlerRegistry build() {
            return new HandlerRegistry(byKey, defaultHandler);
        }
    }
}
