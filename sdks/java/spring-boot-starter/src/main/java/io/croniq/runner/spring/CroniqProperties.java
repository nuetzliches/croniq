package io.croniq.runner.spring;

import io.croniq.runner.config.CroniqRunnerOptions;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import org.springframework.boot.context.properties.ConfigurationProperties;

/**
 * Spring Boot {@code @ConfigurationProperties} binding for the
 * {@code croniq.runner.*} section of {@code application.yml}.
 *
 * <p>Field names mirror {@link CroniqRunnerOptions} so an entry like
 * {@code croniq.runner.poll-timeout: 35s} maps directly to
 * {@link CroniqRunnerOptions.Builder#pollTimeout(Duration)}.
 */
@ConfigurationProperties(prefix = "croniq.runner")
public class CroniqProperties {

    /** Master switch — set to {@code false} to disable the auto-configured runner. */
    private boolean enabled = true;

    private String serverUrl = CroniqRunnerOptions.DEFAULT_SERVER_URL;
    private String runnerId;
    private String runnerIdPrefix = CroniqRunnerOptions.DEFAULT_RUNNER_ID_PREFIX;
    private String runnerDataDir;
    private String apiKey;
    private String bearerToken;
    private List<String> capabilities = new ArrayList<>();
    private List<String> tags = new ArrayList<>();
    private int maxInflight = CroniqRunnerOptions.DEFAULT_MAX_INFLIGHT;
    private Duration pollTimeout = CroniqRunnerOptions.DEFAULT_POLL_TIMEOUT;
    private Duration renewInterval = CroniqRunnerOptions.DEFAULT_RENEW_INTERVAL;
    private Duration drainTimeout = CroniqRunnerOptions.DEFAULT_DRAIN_TIMEOUT;
    private Duration pollRetryDelay = CroniqRunnerOptions.DEFAULT_POLL_RETRY_DELAY;
    private Duration capacityBackoff = CroniqRunnerOptions.DEFAULT_CAPACITY_BACKOFF;

    /**
     * Opt in to a cleartext {@code http} {@link #getServerUrl()} on a non-loopback host.
     * Off by default: such a URL otherwise fails fast at startup, because the API key
     * would be sent in the clear on every poll.
     */
    private boolean allowInsecureHttp;

    public CroniqRunnerOptions toOptions() {
        return CroniqRunnerOptions.builder()
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
                .capacityBackoff(capacityBackoff)
                .build();
    }

    // -------- getters / setters required by Spring's relaxed binding --------

    public boolean isEnabled() {
        return enabled;
    }

    public void setEnabled(boolean enabled) {
        this.enabled = enabled;
    }

    public String getServerUrl() {
        return serverUrl;
    }

    public void setServerUrl(String v) {
        this.serverUrl = v;
    }

    public String getRunnerId() {
        return runnerId;
    }

    public void setRunnerId(String v) {
        this.runnerId = v;
    }

    public String getRunnerIdPrefix() {
        return runnerIdPrefix;
    }

    public void setRunnerIdPrefix(String v) {
        this.runnerIdPrefix = v;
    }

    public String getRunnerDataDir() {
        return runnerDataDir;
    }

    public void setRunnerDataDir(String v) {
        this.runnerDataDir = v;
    }

    public String getApiKey() {
        return apiKey;
    }

    public void setApiKey(String v) {
        this.apiKey = v;
    }

    public String getBearerToken() {
        return bearerToken;
    }

    public void setBearerToken(String v) {
        this.bearerToken = v;
    }

    public List<String> getCapabilities() {
        return capabilities;
    }

    public void setCapabilities(List<String> v) {
        this.capabilities = v == null ? new ArrayList<>() : v;
    }

    public List<String> getTags() {
        return tags;
    }

    public void setTags(List<String> v) {
        this.tags = v == null ? new ArrayList<>() : v;
    }

    public int getMaxInflight() {
        return maxInflight;
    }

    public void setMaxInflight(int v) {
        this.maxInflight = v;
    }

    public Duration getPollTimeout() {
        return pollTimeout;
    }

    public void setPollTimeout(Duration v) {
        this.pollTimeout = v;
    }

    public Duration getRenewInterval() {
        return renewInterval;
    }

    public void setRenewInterval(Duration v) {
        this.renewInterval = v;
    }

    public Duration getDrainTimeout() {
        return drainTimeout;
    }

    public void setDrainTimeout(Duration v) {
        this.drainTimeout = v;
    }

    public Duration getPollRetryDelay() {
        return pollRetryDelay;
    }

    public void setPollRetryDelay(Duration v) {
        this.pollRetryDelay = v;
    }

    public Duration getCapacityBackoff() {
        return capacityBackoff;
    }

    public void setCapacityBackoff(Duration v) {
        this.capacityBackoff = v;
    }

    public boolean isAllowInsecureHttp() {
        return allowInsecureHttp;
    }

    public void setAllowInsecureHttp(boolean v) {
        this.allowInsecureHttp = v;
    }
}
