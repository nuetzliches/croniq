package io.croniq.runner.config;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatIllegalArgumentException;

import ch.qos.logback.classic.Level;
import ch.qos.logback.classic.Logger;
import ch.qos.logback.classic.LoggerContext;
import ch.qos.logback.classic.spi.ILoggingEvent;
import ch.qos.logback.core.read.ListAppender;
import java.time.Duration;
import java.util.List;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.parallel.Execution;
import org.junit.jupiter.api.parallel.ExecutionMode;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.CsvSource;
import org.junit.jupiter.params.provider.ValueSource;
import org.slf4j.LoggerFactory;

/**
 * Base-URL transport security (#440). An {@code https} URL is always accepted; an
 * {@code http} URL only for a loopback host — the documented
 * {@code http://localhost:4000} quickstart path — or behind an explicit
 * {@code allowInsecureHttp(true)}, which additionally logs one loud warning.
 * Enforced in {@code build()}, not on the first request.
 *
 * <p>Runs single-threaded because the warning assertions attach an appender to a
 * shared logger.
 */
@Execution(ExecutionMode.SAME_THREAD)
class ServerUrlsTest {

    private ListAppender<ILoggingEvent> appender;
    private Logger securityLogger;

    @BeforeEach
    void attachAppender() {
        securityLogger = logbackLoggerFor(ServerUrls.class);
        appender = new ListAppender<>();
        appender.start();
        securityLogger.addAppender(appender);
    }

    @AfterEach
    void detachAppender() {
        securityLogger.detachAppender(appender);
        appender.stop();
    }

    @ParameterizedTest
    @ValueSource(
            strings = {
                "https://croniq.example.com",
                "https://croniq.example.com:4000",
                "http://localhost:4000",
                "http://LOCALHOST:4000",
                "http://127.0.0.1:4000",
                "http://127.10.20.30:4000",
                "http://[::1]:4000"
            })
    void runnerOptionsAcceptSecureOrLoopbackUrl(String serverUrl) {
        CroniqRunnerOptions options =
                CroniqRunnerOptions.builder().serverUrl(serverUrl).build();

        assertThat(options.serverUrl().toString()).isEqualTo(serverUrl);
        assertThat(warnings()).isEmpty();
    }

    @ParameterizedTest
    @ValueSource(
            strings = {
                "https://croniq.example.com",
                "http://localhost:4000",
                "http://127.0.0.1:4000",
                "http://[::1]:4000"
            })
    void clientOptionsAcceptSecureOrLoopbackUrl(String serverUrl) {
        CroniqClientOptions options =
                CroniqClientOptions.builder().serverUrl(serverUrl).build();

        assertThat(options.serverUrl().toString()).isEqualTo(serverUrl);
        assertThat(warnings()).isEmpty();
    }

    @ParameterizedTest
    @ValueSource(
            strings = {
                "http://croniq.example.com",
                "http://croniq.example.com:4000",
                "http://10.0.0.5:4000",
                "http://[2001:db8::1]:4000"
            })
    void runnerOptionsRejectNonLoopbackCleartextUrl(String serverUrl) {
        assertThatIllegalArgumentException()
                .isThrownBy(
                        () -> CroniqRunnerOptions.builder().serverUrl(serverUrl).build())
                // Actionable: names the URL and the opt-in.
                .withMessageContaining(serverUrl)
                .withMessageContaining("allowInsecureHttp");
    }

    @ParameterizedTest
    @ValueSource(strings = {"http://croniq.example.com", "http://10.0.0.5:4000", "http://[2001:db8::1]:4000"})
    void clientOptionsRejectNonLoopbackCleartextUrl(String serverUrl) {
        assertThatIllegalArgumentException()
                .isThrownBy(
                        () -> CroniqClientOptions.builder().serverUrl(serverUrl).build())
                .withMessageContaining(serverUrl)
                .withMessageContaining("allowInsecureHttp");
    }

    @Test
    void quickstartDefaultKeepsWorking() {
        assertThat(CroniqRunnerOptions.builder().build().serverUrl().toString())
                .isEqualTo(CroniqRunnerOptions.DEFAULT_SERVER_URL);
        assertThat(CroniqClientOptions.builder().build().serverUrl().toString())
                .isEqualTo(CroniqClientOptions.DEFAULT_SERVER_URL);
        assertThat(warnings()).isEmpty();
    }

    @Test
    void unsupportedSchemeIsRejected() {
        assertThatIllegalArgumentException()
                .isThrownBy(() -> CroniqRunnerOptions.builder()
                        .serverUrl("ftp://croniq.example.com")
                        .build())
                .withMessageContaining("unsupported scheme");
    }

    @Test
    void optInAcceptsCleartextRunnerUrlAndWarnsOnce() {
        CroniqRunnerOptions options = CroniqRunnerOptions.builder()
                .serverUrl("http://croniq.example.com:4000")
                .allowInsecureHttp(true)
                .build();

        assertThat(options.allowInsecureHttp()).isTrue();
        assertThat(warnings()).hasSize(1);
        assertThat(warnings().get(0)).contains("SECURITY").contains("cleartext");
        assertThat(warnings().get(0)).contains("http://croniq.example.com:4000");
    }

    @Test
    void optInAcceptsCleartextClientUrlAndWarnsOnce() {
        CroniqClientOptions options = CroniqClientOptions.builder()
                .serverUrl("http://croniq.example.com:4000")
                .allowInsecureHttp(true)
                .build();

        assertThat(options.allowInsecureHttp()).isTrue();
        assertThat(warnings()).hasSize(1);
        assertThat(warnings().get(0)).contains("SECURITY");
    }

    @Test
    void optInSurvivesToBuilderRoundTrip() {
        CroniqRunnerOptions options =
                CroniqRunnerOptions.builder()
                        .serverUrl("http://croniq.example.com:4000")
                        .allowInsecureHttp(true)
                        .build()
                        .toBuilder()
                        .build();

        assertThat(options.allowInsecureHttp()).isTrue();
    }

    @ParameterizedTest
    @CsvSource({
        "localhost, true",
        "LocalHost, true",
        "127.0.0.1, true",
        "127.255.255.254, true",
        "::1, true",
        "[::1], true",
        "croniq.example.com, false",
        "10.0.0.5, false",
        "2001:db8::1, false",
        "127.0.0, false",
        "127.0.0.999, false"
    })
    void isLoopbackHostClassifiesHosts(String host, boolean expected) {
        assertThat(ServerUrls.isLoopbackHost(host)).isEqualTo(expected);
    }

    @Test
    void isLoopbackHostRejectsNullAndBlank() {
        assertThat(ServerUrls.isLoopbackHost(null)).isFalse();
        assertThat(ServerUrls.isLoopbackHost("")).isFalse();
    }

    /**
     * Resolves the logback logger backing {@code type}.
     *
     * <p>Not a plain cast of {@code LoggerFactory.getLogger(...)}: SLF4J 2 hands a
     * {@code SubstituteLogger} to any thread that calls in while another thread is still
     * initialising the binding, and Gradle runs this module's test classes concurrently —
     * so the cast failed intermittently on CI. Wait for the real {@link LoggerContext}
     * instead.
     */
    private static Logger logbackLoggerFor(Class<?> type) {
        long deadline = System.nanoTime() + Duration.ofSeconds(10).toNanos();
        while (!(LoggerFactory.getILoggerFactory() instanceof LoggerContext)) {
            if (System.nanoTime() > deadline) {
                throw new IllegalStateException("SLF4J did not bind logback-classic in time");
            }
            Thread.onSpinWait();
        }
        return ((LoggerContext) LoggerFactory.getILoggerFactory()).getLogger(type);
    }

    private List<String> warnings() {
        return appender.list.stream()
                .filter(e -> e.getLevel() == Level.WARN)
                .map(ILoggingEvent::getFormattedMessage)
                .toList();
    }
}
