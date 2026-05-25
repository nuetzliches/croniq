package io.croniq.runner.config;

import java.net.URI;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Objects;

/**
 * Runner configuration. Equivalent to the .NET SDK's {@code CroniqRunnerOptions}.
 *
 * <p>Construct via {@link #builder()}; the option names and defaults mirror the
 * .NET SDK's {@code Croniq:Runner} section so the same operator documentation
 * applies to both runtimes.
 */
public final class CroniqRunnerOptions {

    public static final String DEFAULT_SERVER_URL = "http://localhost:4000";
    public static final String DEFAULT_RUNNER_ID_PREFIX = "runner";
    public static final int DEFAULT_MAX_INFLIGHT = 5;
    public static final Duration DEFAULT_POLL_TIMEOUT = Duration.ofSeconds(35);
    public static final Duration DEFAULT_RENEW_INTERVAL = Duration.ofSeconds(15);
    public static final Duration DEFAULT_DRAIN_TIMEOUT = Duration.ofSeconds(30);
    public static final Duration DEFAULT_POLL_RETRY_DELAY = Duration.ofSeconds(5);
    public static final Duration DEFAULT_CAPACITY_BACKOFF = Duration.ofMillis(500);

    private final URI serverUrl;
    private final String runnerId;
    private final String runnerIdPrefix;
    private final String runnerDataDir;
    private final String apiKey;
    private final String bearerToken;
    private final List<String> capabilities;
    private final List<String> tags;
    private final int maxInflight;
    private final Duration pollTimeout;
    private final Duration renewInterval;
    private final Duration drainTimeout;
    private final Duration pollRetryDelay;
    private final Duration capacityBackoff;

    private CroniqRunnerOptions(Builder b) {
        this.serverUrl = b.serverUrl;
        this.runnerId = b.runnerId;
        this.runnerIdPrefix = b.runnerIdPrefix;
        this.runnerDataDir = b.runnerDataDir;
        this.apiKey = b.apiKey;
        this.bearerToken = b.bearerToken;
        this.capabilities = List.copyOf(b.capabilities);
        this.tags = List.copyOf(b.tags);
        this.maxInflight = b.maxInflight;
        this.pollTimeout = b.pollTimeout;
        this.renewInterval = b.renewInterval;
        this.drainTimeout = b.drainTimeout;
        this.pollRetryDelay = b.pollRetryDelay;
        this.capacityBackoff = b.capacityBackoff;
    }

    public URI serverUrl() {
        return serverUrl;
    }

    public String runnerId() {
        return runnerId;
    }

    public String runnerIdPrefix() {
        return runnerIdPrefix;
    }

    public String runnerDataDir() {
        return runnerDataDir;
    }

    public String apiKey() {
        return apiKey;
    }

    public String bearerToken() {
        return bearerToken;
    }

    public List<String> capabilities() {
        return capabilities;
    }

    public List<String> tags() {
        return tags;
    }

    public int maxInflight() {
        return maxInflight;
    }

    public Duration pollTimeout() {
        return pollTimeout;
    }

    public Duration renewInterval() {
        return renewInterval;
    }

    public Duration drainTimeout() {
        return drainTimeout;
    }

    public Duration pollRetryDelay() {
        return pollRetryDelay;
    }

    public Duration capacityBackoff() {
        return capacityBackoff;
    }

    public static Builder builder() {
        return new Builder();
    }

    public Builder toBuilder() {
        return new Builder()
                .serverUrl(serverUrl)
                .runnerId(runnerId)
                .runnerIdPrefix(runnerIdPrefix)
                .runnerDataDir(runnerDataDir)
                .apiKey(apiKey)
                .bearerToken(bearerToken)
                .capabilities(capabilities)
                .tags(tags)
                .maxInflight(maxInflight)
                .pollTimeout(pollTimeout)
                .renewInterval(renewInterval)
                .drainTimeout(drainTimeout)
                .pollRetryDelay(pollRetryDelay)
                .capacityBackoff(capacityBackoff);
    }

    public static final class Builder {
        private URI serverUrl = URI.create(DEFAULT_SERVER_URL);
        private String runnerId;
        private String runnerIdPrefix = DEFAULT_RUNNER_ID_PREFIX;
        private String runnerDataDir;
        private String apiKey;
        private String bearerToken;
        private List<String> capabilities = new ArrayList<>();
        private List<String> tags = new ArrayList<>();
        private int maxInflight = DEFAULT_MAX_INFLIGHT;
        private Duration pollTimeout = DEFAULT_POLL_TIMEOUT;
        private Duration renewInterval = DEFAULT_RENEW_INTERVAL;
        private Duration drainTimeout = DEFAULT_DRAIN_TIMEOUT;
        private Duration pollRetryDelay = DEFAULT_POLL_RETRY_DELAY;
        private Duration capacityBackoff = DEFAULT_CAPACITY_BACKOFF;

        public Builder serverUrl(URI v) {
            this.serverUrl = Objects.requireNonNull(v, "serverUrl");
            return this;
        }

        public Builder serverUrl(String v) {
            return serverUrl(URI.create(v));
        }

        public Builder runnerId(String v) {
            this.runnerId = v;
            return this;
        }

        public Builder runnerIdPrefix(String v) {
            this.runnerIdPrefix = Objects.requireNonNullElse(v, DEFAULT_RUNNER_ID_PREFIX);
            return this;
        }

        public Builder runnerDataDir(String v) {
            this.runnerDataDir = v;
            return this;
        }

        public Builder apiKey(String v) {
            this.apiKey = v;
            return this;
        }

        public Builder bearerToken(String v) {
            this.bearerToken = v;
            return this;
        }

        public Builder capabilities(List<String> v) {
            this.capabilities = v == null ? Collections.emptyList() : new ArrayList<>(v);
            return this;
        }

        public Builder tags(List<String> v) {
            this.tags = v == null ? Collections.emptyList() : new ArrayList<>(v);
            return this;
        }

        public Builder maxInflight(int v) {
            if (v < 1 || v > 1024) {
                throw new IllegalArgumentException("maxInflight must be in [1, 1024], got " + v);
            }
            this.maxInflight = v;
            return this;
        }

        public Builder pollTimeout(Duration v) {
            this.pollTimeout = Objects.requireNonNull(v, "pollTimeout");
            return this;
        }

        public Builder renewInterval(Duration v) {
            this.renewInterval = Objects.requireNonNull(v, "renewInterval");
            return this;
        }

        public Builder drainTimeout(Duration v) {
            this.drainTimeout = Objects.requireNonNull(v, "drainTimeout");
            return this;
        }

        public Builder pollRetryDelay(Duration v) {
            this.pollRetryDelay = Objects.requireNonNull(v, "pollRetryDelay");
            return this;
        }

        public Builder capacityBackoff(Duration v) {
            this.capacityBackoff = Objects.requireNonNull(v, "capacityBackoff");
            return this;
        }

        public CroniqRunnerOptions build() {
            return new CroniqRunnerOptions(this);
        }
    }
}
