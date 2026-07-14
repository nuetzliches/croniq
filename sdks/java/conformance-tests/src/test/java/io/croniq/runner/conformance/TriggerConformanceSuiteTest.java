package io.croniq.runner.conformance;

import static org.junit.jupiter.api.DynamicTest.dynamicTest;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Set;
import java.util.stream.Stream;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.TestFactory;

/**
 * Drives every trigger (producer) YAML case under
 * {@code sdks/conformance/cases-trigger/} against the Java SDK's
 * {@link io.croniq.runner.CroniqTriggerClient}. Each case becomes one dynamic
 * test so the IDE / CI surfaces it by name when it fails.
 *
 * <p>The {@code cases-trigger/} directory ships with the shared conformance
 * suite (<a href="https://github.com/nuetzliches/croniq/issues/287">#287</a>).
 * Until that lands in a given checkout the directory is absent, so this factory
 * emits a single placeholder test rather than failing — the trigger suite lights
 * up automatically once the cases are present. Uses a dynamic {@link TestFactory}
 * (not a parameterized test) precisely so an empty case set is not an error.
 */
class TriggerConformanceSuiteTest {

    private static final Path CASES_DIR =
            Path.of("..", "..", "conformance", "cases-trigger").toAbsolutePath().normalize();

    /** Cases the Java trigger client implements. Expanded as the shared suite grows. */
    private static final Set<String> SCOPE = Set.of(
            "01-trigger-minimal.yaml",
            "02-trigger-full-request.yaml",
            "03-trigger-metadata.yaml",
            "04-trigger-require-prefer.yaml",
            "05-trigger-timeout.yaml",
            "06-trigger-auth-apikey.yaml",
            "07-trigger-dedup-hit.yaml",
            "08-trigger-dedup-flag-absent.yaml",
            "09-trigger-idempotency-oversized.yaml",
            "10-trigger-server-error.yaml",
            "11-trigger-queue-overflow.yaml");

    @TestFactory
    Stream<DynamicTest> triggerConformance() throws Exception {
        if (!Files.isDirectory(CASES_DIR)) {
            return Stream.of(
                    dynamicTest("cases-trigger absent (pending #287) — trigger conformance skipped", () -> {}));
        }
        List<Path> cases;
        try (Stream<Path> files = Files.list(CASES_DIR)) {
            cases = files.filter(p -> p.toString().endsWith(".yaml"))
                    .filter(p -> SCOPE.contains(p.getFileName().toString()))
                    .sorted()
                    .toList();
        }
        if (cases.isEmpty()) {
            return Stream.of(dynamicTest("no in-scope trigger cases found", () -> {}));
        }
        return cases.stream()
                .map(p -> dynamicTest(p.getFileName().toString(), () -> {
                    TriggerCaseSpec spec = TriggerCaseLoader.load(p);
                    new TriggerConformanceRunner().run(spec);
                }));
    }
}
