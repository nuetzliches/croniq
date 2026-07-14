package io.croniq.runner.config;

import java.net.URI;
import java.time.Duration;
import java.util.Objects;

/**
 * Configuration for the producer-side trigger client
 * ({@link io.croniq.runner.CroniqTriggerClient}).
 *
 * <p>Deliberately separate from {@link CroniqRunnerOptions}: triggering requires
 * the {@code jobs:trigger} (or {@code admin}) scope, which is distinct from the
 * runner's poll scopes — the trigger client therefore carries its own
 * credentials instead of assuming the runner's. Mirrors the .NET SDK's
 * {@code CroniqClientOptions} (config section {@code Croniq:Client}) so the same
 * operator documentation applies to both runtimes.
 *
 * <p>Construct via {@link #builder()}. {@code apiKey} takes precedence over
 * {@code bearerToken} when both are set, matching the runner client and the
 * conformance suite's expectations.
 */
public final class CroniqClientOptions {

    public static final String DEFAULT_SERVER_URL = "http://localhost:4000";
    public static final Duration DEFAULT_REQUEST_TIMEOUT = Duration.ofSeconds(30);

    private final URI serverUrl;
    private final String apiKey;
    private final String bearerToken;
    private final Duration requestTimeout;

    private CroniqClientOptions(Builder b) {
        this.serverUrl = b.serverUrl;
        this.apiKey = b.apiKey;
        this.bearerToken = b.bearerToken;
        this.requestTimeout = b.requestTimeout;
    }

    /** Base URL of the Croniq server, e.g. {@code http://localhost:4000}. */
    public URI serverUrl() {
        return serverUrl;
    }

    /**
     * API key sent as {@code Authorization: ApiKey <key>}. Needs the
     * {@code jobs:trigger} or {@code admin} scope. Takes precedence over
     * {@link #bearerToken()} when both are set.
     */
    public String apiKey() {
        return apiKey;
    }

    /** Bearer token sent as {@code Authorization: Bearer <token>}. */
    public String bearerToken() {
        return bearerToken;
    }

    /** Per-request timeout for trigger calls. */
    public Duration requestTimeout() {
        return requestTimeout;
    }

    public static Builder builder() {
        return new Builder();
    }

    public Builder toBuilder() {
        return new Builder()
                .serverUrl(serverUrl)
                .apiKey(apiKey)
                .bearerToken(bearerToken)
                .requestTimeout(requestTimeout);
    }

    public static final class Builder {
        private URI serverUrl = URI.create(DEFAULT_SERVER_URL);
        private String apiKey;
        private String bearerToken;
        private Duration requestTimeout = DEFAULT_REQUEST_TIMEOUT;

        private Builder() {}

        public Builder serverUrl(URI v) {
            this.serverUrl = Objects.requireNonNull(v, "serverUrl");
            return this;
        }

        public Builder serverUrl(String v) {
            return serverUrl(URI.create(v));
        }

        public Builder apiKey(String v) {
            this.apiKey = v;
            return this;
        }

        public Builder bearerToken(String v) {
            this.bearerToken = v;
            return this;
        }

        public Builder requestTimeout(Duration v) {
            this.requestTimeout = Objects.requireNonNull(v, "requestTimeout");
            return this;
        }

        public CroniqClientOptions build() {
            return new CroniqClientOptions(this);
        }
    }
}
