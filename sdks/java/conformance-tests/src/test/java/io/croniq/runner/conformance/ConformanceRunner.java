package io.croniq.runner.conformance;

import static org.assertj.core.api.Assertions.fail;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import io.croniq.runner.CroniqAuthFailedException;
import io.croniq.runner.CroniqOwnershipDeniedException;
import io.croniq.runner.CroniqPollInstanceConflictException;
import io.croniq.runner.CroniqRunner;
import io.croniq.runner.config.CroniqRunnerOptions;
import java.time.Duration;
import java.util.List;
import org.awaitility.Awaitility;

/**
 * Drives one conformance case end-to-end: stand up the mock server, build
 * a {@link CroniqRunner} with the case's config, run it on a worker thread,
 * poll for expectations, then assert the recorded request stream.
 *
 * <p>Lifetime: each instance is single-use. Call {@link #run(CaseSpec)} once
 * per case.
 */
final class ConformanceRunner {

    private static final ObjectMapper JSON = new ObjectMapper();

    void run(CaseSpec spec) throws Exception {
        try (MockServerHarness server = new MockServerHarness(spec.serverScript())) {
            server.start();
            CroniqRunnerOptions options = buildOptions(spec.runnerConfig(), server.baseUrl());

            CroniqRunner.Builder builder = CroniqRunner.builder().options(options);
            HandlerSentinels.applyTo(builder, spec.handlers());

            CroniqRunner runner = builder.build();
            Thread loop = Thread.ofVirtual().name("conformance-runner-loop").start(() -> {
                try {
                    runner.run();
                } catch (InterruptedException ignored) {
                    // expected on close()
                } catch (CroniqOwnershipDeniedException
                        | CroniqPollInstanceConflictException
                        | CroniqAuthFailedException ignored) {
                    // Expected for cases 15, 16 and 17: a 403 on poll is
                    // permanent, a streak of 409s exhausts the conflict
                    // ceiling, and a streak of 401s exhausts the auth ceiling
                    // — the SDK is contractually required to stop in all
                    // three. The HTTP-count assertions are what prove it
                    // actually did; a case that doesn't anticipate this exit
                    // still fails on min_count/max_count.
                }
            });
            try {
                // Optional mid-run shutdown directive (drain cases — PR-3).
                if (spec.shutdownAfterMs() != null) {
                    Thread.sleep(spec.shutdownAfterMs());
                    runner.close();
                }

                // Wait for the expectations to be satisfiable or the case's
                // duration cap, whichever comes first. The wait is the
                // "settling" period — there's no early-exit ack from the
                // SDK so we just let the loop run until the deadline.
                int durationMaxMs = spec.expectations().durationMaxMs() == null
                        ? 5_000
                        : spec.expectations().durationMaxMs();
                if (hasMaxCount(spec)) {
                    // A max_count is a "ceiling over a time window" assertion.
                    // Exiting as soon as the lower bounds are met would let a
                    // runner that violates the ceiling right after our exit
                    // pass trivially, so burn the full window. Mirrors the
                    // .NET / Go / Python / TypeScript bindings.
                    Thread.sleep(durationMaxMs);
                } else {
                    Awaitility.await()
                            .atMost(Duration.ofMillis(durationMaxMs))
                            .pollInterval(Duration.ofMillis(50))
                            .until(() -> expectationsLikelyMet(spec, server.recorded()));
                }
            } finally {
                runner.close();
                loop.join(Duration.ofSeconds(5));
            }

            assertExpectations(spec, server.recorded());
        }
    }

    private static CroniqRunnerOptions buildOptions(CaseSpec.RunnerConfig rc, String serverUrl) {
        var b = CroniqRunnerOptions.builder().serverUrl(serverUrl);
        if (rc == null) {
            return b.build();
        }
        if (rc.runnerId() != null) {
            b.runnerId(rc.runnerId());
        }
        if (rc.runnerIdPrefix() != null) {
            b.runnerIdPrefix(rc.runnerIdPrefix());
        }
        if (rc.capabilities() != null) {
            b.capabilities(rc.capabilities());
        }
        if (rc.tags() != null) {
            b.tags(rc.tags());
        }
        if (rc.maxInflight() != null) {
            b.maxInflight(rc.maxInflight());
        }
        if (rc.apiKey() != null) {
            b.apiKey(rc.apiKey());
        }
        if (rc.bearerToken() != null) {
            b.bearerToken(rc.bearerToken());
        }
        if (rc.pollTimeoutMs() != null) {
            b.pollTimeout(Duration.ofMillis(rc.pollTimeoutMs()));
        }
        if (rc.renewIntervalMs() != null) {
            b.renewInterval(Duration.ofMillis(rc.renewIntervalMs()));
        }
        if (rc.drainTimeoutMs() != null) {
            b.drainTimeout(Duration.ofMillis(rc.drainTimeoutMs()));
        }
        if (rc.pollRetryDelayMs() != null) {
            b.pollRetryDelay(Duration.ofMillis(rc.pollRetryDelayMs()));
        }
        if (rc.capacityBackoffMs() != null) {
            b.capacityBackoff(Duration.ofMillis(rc.capacityBackoffMs()));
        }
        if (rc.maxConsecutivePollConflicts() != null) {
            b.maxConsecutivePollConflicts(rc.maxConsecutivePollConflicts());
        }
        if (rc.maxConsecutiveAuthFailures() != null) {
            b.maxConsecutiveAuthFailures(rc.maxConsecutiveAuthFailures());
        }
        return b.build();
    }

    /** True when any expectation carries a {@code max_count} ceiling. */
    private static boolean hasMaxCount(CaseSpec spec) {
        if (spec.expectations() == null || spec.expectations().http() == null) {
            return false;
        }
        return spec.expectations().http().stream().anyMatch(e -> e.maxCount() != null);
    }

    /**
     * Returns true once every expectation's {@code min_count} / {@code exact_count}
     * lower bound is satisfied. Only consulted for cases without a
     * {@code max_count} — those burn the full window instead, see
     * {@link #hasMaxCount(CaseSpec)}.
     */
    private static boolean expectationsLikelyMet(CaseSpec spec, List<MockServerHarness.RecordedRequest> recorded) {
        if (spec.expectations() == null || spec.expectations().http() == null) {
            return true;
        }
        for (var e : spec.expectations().http()) {
            long n = recorded.stream()
                    .filter(r -> r.matches(e.method(), e.path()))
                    .count();
            if (e.exactCount() != null && n < e.exactCount()) {
                return false;
            }
            if (e.minCount() != null && n < e.minCount()) {
                return false;
            }
        }
        return true;
    }

    private static void assertExpectations(CaseSpec spec, List<MockServerHarness.RecordedRequest> recorded) {
        if (spec.expectations() == null || spec.expectations().http() == null) {
            return;
        }
        for (var e : spec.expectations().http()) {
            var matches = recorded.stream()
                    .filter(r -> r.matches(e.method(), e.path()))
                    .toList();
            int n = matches.size();
            if (e.exactCount() != null && n != e.exactCount()) {
                fail("Expected %s %s exact_count=%d, got %d. Recorded: %s"
                        .formatted(e.method(), e.path(), e.exactCount(), n, summary(recorded)));
            }
            if (e.minCount() != null && n < e.minCount()) {
                fail("Expected %s %s min_count=%d, got %d".formatted(e.method(), e.path(), e.minCount(), n));
            }
            if (e.maxCount() != null && n > e.maxCount()) {
                fail("Expected %s %s max_count=%d, got %d".formatted(e.method(), e.path(), e.maxCount(), n));
            }
            if (e.headers() != null && !e.headers().isEmpty()) {
                if (matches.isEmpty()) {
                    fail("Headers expected on %s %s but no requests recorded".formatted(e.method(), e.path()));
                }
                var first = matches.get(0);
                for (var h : e.headers().entrySet()) {
                    String actual = first.headers().get(h.getKey().toLowerCase());
                    if (actual == null) {
                        fail("Missing header '%s' on %s %s. Headers seen: %s"
                                .formatted(
                                        h.getKey(),
                                        e.method(),
                                        e.path(),
                                        first.headers().keySet()));
                    }
                    if ("*".equals(h.getValue())) {
                        if (actual.isEmpty()) {
                            fail("Header '%s' expected non-empty (*) but was empty".formatted(h.getKey()));
                        }
                    } else if (!h.getValue().equals(actual)) {
                        fail("Header '%s' expected '%s' but was '%s'".formatted(h.getKey(), h.getValue(), actual));
                    }
                }
            }
            if (e.bodyMatch() != null) {
                if (matches.isEmpty()) {
                    fail("body_match expected on %s %s but no requests recorded".formatted(e.method(), e.path()));
                }
                var first = matches.get(0);
                JsonNode actualBody;
                try {
                    actualBody = first.body().isEmpty() ? JSON.nullNode() : JSON.readTree(first.body());
                } catch (Exception ex) {
                    fail("Could not parse JSON body for %s %s: %s".formatted(e.method(), e.path(), first.body()));
                    return;
                }
                String err = BodyMatcher.match(e.bodyMatch(), actualBody);
                if (err != null) {
                    fail("body_match failed on %s %s: %s. Actual: %s"
                            .formatted(e.method(), e.path(), err, first.body()));
                }
            }
        }
    }

    private static String summary(List<MockServerHarness.RecordedRequest> recorded) {
        StringBuilder sb = new StringBuilder().append('[');
        for (var r : recorded) {
            sb.append(r.method()).append(' ').append(r.path()).append(", ");
        }
        if (sb.length() > 1) {
            sb.setLength(sb.length() - 2);
        }
        return sb.append(']').toString();
    }
}
