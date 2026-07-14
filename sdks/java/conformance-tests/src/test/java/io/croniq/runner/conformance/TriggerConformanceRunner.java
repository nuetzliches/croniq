package io.croniq.runner.conformance;

import static org.assertj.core.api.Assertions.fail;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import io.croniq.runner.CroniqTriggerClient;
import io.croniq.runner.TriggerRequest;
import io.croniq.runner.TriggerResult;
import io.croniq.runner.config.CroniqClientOptions;
import java.util.List;

/**
 * Drives one trigger (producer) conformance case: stand up the scripted mock
 * server, build a {@link CroniqTriggerClient} with the case's credentials, make
 * each scripted {@code trigger(...)} call in order asserting its expected
 * outcome (a parsed response or an error), then assert the recorded request
 * stream against {@code expectations.http}.
 *
 * <p>Lifetime: single-use. Call {@link #run(TriggerCaseSpec)} once per case.
 */
final class TriggerConformanceRunner {

    private static final ObjectMapper JSON = new ObjectMapper();

    void run(TriggerCaseSpec spec) throws Exception {
        try (MockServerHarness server = new MockServerHarness(spec.serverScript())) {
            server.start();
            CroniqTriggerClient client = new CroniqTriggerClient(buildOptions(spec.triggerConfig(), server.baseUrl()));

            for (TriggerCaseSpec.TriggerCall call : spec.triggerCalls()) {
                invokeAndAssert(client, call);
            }

            assertExpectations(spec.expectations(), server.recorded());
        }
    }

    private static CroniqClientOptions buildOptions(TriggerCaseSpec.TriggerConfig cfg, String serverUrl) {
        var b = CroniqClientOptions.builder().serverUrl(serverUrl);
        if (cfg != null) {
            if (cfg.apiKey() != null) {
                b.apiKey(cfg.apiKey());
            }
            if (cfg.bearerToken() != null) {
                b.bearerToken(cfg.bearerToken());
            }
        }
        return b.build();
    }

    private static void invokeAndAssert(CroniqTriggerClient client, TriggerCaseSpec.TriggerCall call) {
        var r = call.request();
        TriggerRequest request = TriggerRequest.builder(r.jobKey())
                .metadata(r.metadata())
                .require(r.require())
                .prefer(r.prefer())
                .timeout(r.timeout())
                .idempotencyKey(r.idempotencyKey())
                .build();

        boolean expectError = call.expect() != null
                && call.expect().error() != null
                && call.expect().error();

        TriggerResult result = null;
        RuntimeException thrown = null;
        try {
            result = client.trigger(request);
        } catch (RuntimeException ex) {
            thrown = ex;
        }

        if (expectError) {
            if (thrown == null) {
                fail("trigger(%s): expected an error but got result %s".formatted(r.jobKey(), result));
            }
            return;
        }
        if (thrown != null) {
            fail("trigger(%s): expected a response but the client raised %s".formatted(r.jobKey(), thrown));
        }
        assertResponse(r.jobKey(), call.expect().response(), result);
    }

    private static void assertResponse(
            String jobKey, TriggerCaseSpec.TriggerCall.Expect.Response expected, TriggerResult actual) {
        if (expected == null) {
            return;
        }
        if (expected.executionId() != null) {
            if ("*".equals(expected.executionId())) {
                if (actual.executionId() == null || actual.executionId().isEmpty()) {
                    fail("trigger(%s): expected non-empty execution_id (*) but was '%s'"
                            .formatted(jobKey, actual.executionId()));
                }
            } else if (!expected.executionId().equals(actual.executionId())) {
                fail("trigger(%s): expected execution_id '%s' but got '%s'"
                        .formatted(jobKey, expected.executionId(), actual.executionId()));
            }
        }
        if (expected.queued() != null && expected.queued() != actual.queued()) {
            fail("trigger(%s): expected queued=%d but got %d".formatted(jobKey, expected.queued(), actual.queued()));
        }
        if (expected.deduplicated() != null && expected.deduplicated() != actual.deduplicated()) {
            fail("trigger(%s): expected deduplicated=%s but got %s"
                    .formatted(jobKey, expected.deduplicated(), actual.deduplicated()));
        }
    }

    private static void assertExpectations(
            TriggerCaseSpec.Expectations expectations, List<MockServerHarness.RecordedRequest> recorded) {
        if (expectations == null || expectations.http() == null) {
            return;
        }
        for (var e : expectations.http()) {
            var matches = recorded.stream()
                    .filter(r -> r.matches(e.method(), e.path()))
                    .toList();
            int n = matches.size();
            if (e.exactCount() != null && n != e.exactCount()) {
                fail("Expected %s %s exact_count=%d, got %d".formatted(e.method(), e.path(), e.exactCount(), n));
            }
            if (e.minCount() != null && n < e.minCount()) {
                fail("Expected %s %s min_count=%d, got %d".formatted(e.method(), e.path(), e.minCount(), n));
            }
            if (e.maxCount() != null && n > e.maxCount()) {
                fail("Expected %s %s max_count=%d, got %d".formatted(e.method(), e.path(), e.maxCount(), n));
            }
            assertHeaders(e, matches);
            assertBody(e, matches);
        }
    }

    private static void assertHeaders(
            TriggerCaseSpec.Expectations.HttpExpectation e, List<MockServerHarness.RecordedRequest> matches) {
        if (e.headers() == null || e.headers().isEmpty()) {
            return;
        }
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

    private static void assertBody(
            TriggerCaseSpec.Expectations.HttpExpectation e, List<MockServerHarness.RecordedRequest> matches) {
        boolean hasBodyCheck = e.bodyMatch() != null
                || (e.bodyAbsent() != null && !e.bodyAbsent().isEmpty());
        if (!hasBodyCheck) {
            return;
        }
        if (matches.isEmpty()) {
            fail("body expectation on %s %s but no requests recorded".formatted(e.method(), e.path()));
        }
        var first = matches.get(0);
        JsonNode body;
        try {
            body = first.body().isEmpty() ? JSON.nullNode() : JSON.readTree(first.body());
        } catch (Exception ex) {
            fail("Could not parse JSON body for %s %s: %s".formatted(e.method(), e.path(), first.body()));
            return;
        }
        if (e.bodyMatch() != null) {
            String err = BodyMatcher.match(e.bodyMatch(), body);
            if (err != null) {
                fail("body_match failed on %s %s: %s. Actual: %s".formatted(e.method(), e.path(), err, first.body()));
            }
        }
        if (e.bodyAbsent() != null) {
            for (String key : e.bodyAbsent()) {
                if (body.has(key)) {
                    fail("body_absent violated on %s %s: key '%s' present. Actual: %s"
                            .formatted(e.method(), e.path(), key, first.body()));
                }
            }
        }
    }
}
