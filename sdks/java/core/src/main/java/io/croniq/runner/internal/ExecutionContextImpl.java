package io.croniq.runner.internal;

import com.fasterxml.jackson.databind.JsonNode;
import io.croniq.runner.handler.CroniqCancellation;
import io.croniq.runner.handler.CroniqExecutionContext;
import io.croniq.runner.handler.CroniqLogWriter;
import java.time.Duration;
import java.time.Instant;
import java.util.List;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Package-private context implementation. Public surface lives on the
 * {@link CroniqExecutionContext} interface so users have a stable API across
 * PRs while we add fields (log writer, OTel scope, …) internally.
 */
final class ExecutionContextImpl implements CroniqExecutionContext {

    private final String executionId;
    private final String jobKey;
    private final Instant scheduledFor;
    private final int attempt;
    private final JsonNode metadata;
    private final Duration timeout;
    private final String runnerId;
    private final List<String> runnerTags;
    private final Logger logger;
    private final CroniqCancellation cancellation;
    private final CroniqLogWriter logWriter;

    ExecutionContextImpl(
            String executionId,
            String jobKey,
            Instant scheduledFor,
            int attempt,
            JsonNode metadata,
            Duration timeout,
            String runnerId,
            List<String> runnerTags,
            CroniqCancellation cancellation,
            CroniqLogWriter logWriter) {
        this.executionId = executionId;
        this.jobKey = jobKey;
        this.scheduledFor = scheduledFor;
        this.attempt = attempt;
        this.metadata = metadata;
        this.timeout = timeout;
        this.runnerId = runnerId;
        this.runnerTags = runnerTags;
        this.cancellation = cancellation;
        this.logWriter = logWriter;
        // PR-4 will install an MDC adapter; for now a plain per-job logger is
        // enough for the conformance sentinels.
        this.logger = LoggerFactory.getLogger("io.croniq.runner.job." + jobKey);
    }

    @Override
    public String executionId() {
        return executionId;
    }

    @Override
    public String jobKey() {
        return jobKey;
    }

    @Override
    public Instant scheduledFor() {
        return scheduledFor;
    }

    @Override
    public int attempt() {
        return attempt;
    }

    @Override
    public JsonNode metadata() {
        return metadata;
    }

    @Override
    public Duration timeout() {
        return timeout;
    }

    @Override
    public String runnerId() {
        return runnerId;
    }

    @Override
    public List<String> runnerTags() {
        return runnerTags;
    }

    @Override
    public Logger logger() {
        return logger;
    }

    @Override
    public CroniqCancellation cancellation() {
        return cancellation;
    }

    @Override
    public CroniqLogWriter logWriter() {
        return logWriter;
    }
}
