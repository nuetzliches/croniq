package io.croniq.runner.conformance;

import static org.assertj.core.api.Assertions.assertThat;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;
import java.util.stream.Stream;
import org.junit.jupiter.api.Assumptions;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.Arguments;
import org.junit.jupiter.params.provider.MethodSource;

/**
 * Drives every trigger (producer) YAML case under
 * {@code sdks/conformance/cases-trigger/} against the Java SDK's
 * {@link io.croniq.runner.CroniqTriggerClient}. Each case becomes one JUnit
 * test so the IDE / CI surfaces it by name when it fails.
 *
 * <p>Same polarity as {@link ConformanceSuiteTest}, and for the same reason
 * (#453): every case in {@code cases-trigger/} runs by default, and
 * {@link #UNSUPPORTED} is an exclusion list that has to justify itself and is
 * checked against the corpus so it cannot rot.
 *
 * <p>This suite previously used a dynamic {@code @TestFactory} that emitted a
 * green placeholder test when {@code cases-trigger/} was absent or filtered
 * down to nothing — a deliberate concession while the shared trigger corpus was
 * still pending (#287). That corpus has landed, so an absent or empty directory
 * is now a broken checkout rather than an expected state, and it fails.
 */
class TriggerConformanceSuiteTest {

    private static final Path CASES_DIR =
            Path.of("..", "..", "conformance", "cases-trigger").toAbsolutePath().normalize();

    /**
     * Trigger cases the Java client deliberately does not run, mapped to the
     * reason why. See {@link ConformanceSuiteTest#UNSUPPORTED} — same contract:
     * absent means it runs, present means it is reported as skipped with the
     * reason, and a stale entry fails the suite.
     *
     * <p>Empty as of #453: the Java trigger client runs the whole corpus.
     */
    private static final Map<String, String> UNSUPPORTED = Map.of();

    /** Every {@code *.yaml} in the trigger corpus, sorted. */
    private static List<Path> corpus() throws IOException {
        if (!Files.isDirectory(CASES_DIR)) {
            throw new IllegalStateException("Trigger cases dir not found: " + CASES_DIR);
        }
        try (Stream<Path> files = Files.list(CASES_DIR)) {
            return files.filter(p -> p.toString().endsWith(".yaml")).sorted().toList();
        }
    }

    static Stream<Arguments> triggerCases() throws Exception {
        List<Path> corpus = corpus();
        if (corpus.isEmpty()) {
            throw new IllegalStateException("No trigger conformance cases found under " + CASES_DIR);
        }
        return corpus.stream().map(p -> Arguments.of(label(p), p)).toList().stream();
    }

    /** @see ConformanceSuiteTest#label(Path) */
    private static String label(Path caseFile) {
        String name = caseFile.getFileName().toString();
        String reason = UNSUPPORTED.get(name);
        return reason == null ? name : name + " [SKIPPED: " + reason + "]";
    }

    /** @see ConformanceSuiteTest#unsupportedEntriesReferenceExistingCases() */
    @Test
    void unsupportedEntriesReferenceExistingCases() throws Exception {
        Set<String> present = new TreeSet<>();
        for (Path p : corpus()) {
            present.add(p.getFileName().toString());
        }
        assertThat(present).as("trigger conformance corpus under %s", CASES_DIR).isNotEmpty();
        assertThat(UNSUPPORTED.keySet())
                .as("every UNSUPPORTED entry must name a case that exists in %s", CASES_DIR)
                .allSatisfy(name -> assertThat(present).contains(name));
    }

    @ParameterizedTest(name = "{0}")
    @MethodSource("triggerCases")
    void triggerConformanceCase(String label, Path caseFile) throws Exception {
        String reason = UNSUPPORTED.get(caseFile.getFileName().toString());
        if (reason != null) {
            Assumptions.abort("Not supported by the Java trigger client: " + reason);
        }

        TriggerCaseSpec spec = TriggerCaseLoader.load(caseFile);
        new TriggerConformanceRunner().run(spec);
    }
}
