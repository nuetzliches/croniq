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
    public static final int DEFAULT_MAX_CONSECUTIVE_POLL_CONFLICTS = 3;

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
    private final int maxConsecutivePollConflicts;
    private final boolean allowInsecureHttp;

    private CroniqRunnerOptions(Builder b) {
        this.allowInsecureHttp = b.allowInsecureHttp;
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
        this.maxConsecutivePollConflicts = b.maxConsecutivePollConflicts;
    }

    /**
     * Base URL of the Croniq server.
     *
     * <p>Must be {@code https} unless the host is loopback ({@code localhost},
     * {@code 127.0.0.0/8}, {@code ::1}) — the API key rides along on every request and
     * would otherwise travel in cleartext. See {@link #allowInsecureHttp()}.
     */
    public URI serverUrl() {
        return serverUrl;
    }

    /**
     * Whether a cleartext {@code http} {@link #serverUrl()} on a non-loopback host was
     * explicitly opted in to. Off by default: such a URL is otherwise refused by
     * {@link Builder#build()}.
     */
    public boolean allowInsecureHttp() {
        return allowInsecureHttp;
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

    /**
     * How many consecutive {@code 409 Conflict} responses from
     * {@code POST /v1/work/poll} the runner tolerates before {@link
     * io.croniq.runner.CroniqRunner#run()} throws {@link
     * io.croniq.runner.CroniqPollInstanceConflictException}.
     *
     * <p>A sustained {@code 409} means a second process is registered under the same
     * {@code runner_id} and no amount of retrying fixes that. The counter resets on a
     * successful poll or on any non-409 failure (5xx, network, timeout), which say
     * nothing about instance ownership. Defaults to
     * {@link #DEFAULT_MAX_CONSECUTIVE_POLL_CONFLICTS}.
     */
    public int maxConsecutivePollConflicts() {
        return maxConsecutivePollConflicts;
    }

    public static Builder builder() {
        return new Builder();
    }

    public Builder toBuilder() {
        return new Builder()
                .allowInsecureHttp(allowInsecureHttp)
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
                .maxConsecutivePollConflicts(maxConsecutivePollConflicts)
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
        private int maxConsecutivePollConflicts = DEFAULT_MAX_CONSECUTIVE_POLL_CONFLICTS;
        private Duration capacityBackoff = DEFAULT_CAPACITY_BACKOFF;
        private boolean allowInsecureHttp;

        public Builder serverUrl(URI v) {
            this.serverUrl = Objects.requireNonNull(v, "serverUrl");
            return this;
        }

        public Builder serverUrl(String v) {
            return serverUrl(URI.create(v));
        }

        /**
         * Opts in to a cleartext {@code http} {@code serverUrl} on a non-loopback host.
         *
         * <p>Off by default: such a URL is otherwise refused by {@link #build()}. With the
         * opt-in the runner starts but logs one loud warning — the API key then travels in
         * cleartext on every poll, and through any HTTP proxy the environment configures.
         * Lab and staging only; never production.
         *
         * @param v whether cleartext HTTP is accepted
         * @return this builder
         */
        public Builder allowInsecureHttp(boolean v) {
            this.allowInsecureHttp = v;
            return this;
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

        /**
         * Range-checked like {@link #maxInflight(int)}: 0 would make the runner exit on
         * its very first {@code 409}, which reads as a crash-loop rather than the
         * duplicate deployment it actually is.
         */
        public Builder maxConsecutivePollConflicts(int v) {
            if (v < 1 || v > 100) {
                throw new IllegalArgumentException("maxConsecutivePollConflicts must be in [1, 100], got " + v);
            }
            this.maxConsecutivePollConflicts = v;
            return this;
        }

        public CroniqRunnerOptions build() {
            // Transport security (#440): fail fast on a base URL that would put the
            // API key on the wire in the clear, rather than on the first poll.
            ServerUrls.validate(serverUrl, allowInsecureHttp, "CroniqRunnerOptions");
            return new CroniqRunnerOptions(this);
        }
    }
}
