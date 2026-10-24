package io.croniq.runner.internal;

import static org.assertj.core.api.Assertions.assertThat;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import ch.qos.logback.classic.Level;
import ch.qos.logback.classic.Logger;
import ch.qos.logback.classic.LoggerContext;
import ch.qos.logback.classic.spi.ILoggingEvent;
import ch.qos.logback.core.read.ListAppender;
import io.croniq.runner.config.CroniqRunnerOptions;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import org.slf4j.LoggerFactory;

class RunnerIdentityResolverTest {

    @Test
    void unwritableDataDirIsReportedAtWarn(@TempDir Path tmp) throws Exception {
        assumeTrue(System.getenv("RUNNER_ID") == null, "RUNNER_ID set in the environment");
        // A regular file where the data directory should be: createDirectories fails.
        Path notADir = Files.writeString(tmp.resolve("state"), "x");
        var options = CroniqRunnerOptions.builder()
                .runnerIdPrefix("runner")
                .runnerDataDir(notADir.toString())
                .build();

        Logger logger = ((LoggerContext) LoggerFactory.getILoggerFactory()).getLogger(RunnerIdentityResolver.class);
        ListAppender<ILoggingEvent> appender = new ListAppender<>();
        appender.start();
        logger.addAppender(appender);
        try {
            String id = new RunnerIdentityResolver(options).resolve();
            assertThat(id).startsWith("runner-");
        } finally {
            logger.detachAppender(appender);
        }

        // Review 2026-10-24, K-6: this was DEBUG, so a container without a
        // writable state dir changed runner_id on every restart silently.
        assertThat(appender.list).anySatisfy(e -> {
            assertThat(e.getLevel()).isEqualTo(Level.WARN);
            assertThat(e.getFormattedMessage()).contains(notADir.toString()).contains("CRONIQ_RUNNER_DATA_DIR");
        });
    }
}
