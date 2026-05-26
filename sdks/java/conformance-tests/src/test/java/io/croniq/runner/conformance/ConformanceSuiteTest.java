package io.croniq.runner.conformance;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Set;
import java.util.stream.Stream;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.Arguments;
import org.junit.jupiter.params.provider.MethodSource;

/**
 * Drives every YAML case under {@code sdks/conformance/cases/} against the
 * Java SDK. Each case becomes one JUnit test so the IDE / CI surfaces it by
 * name when it fails.
 *
 * <p>PR-2 implements cases 01-04 (poll-empty, poll-single-success,
 * handler-failure, cancel-via-poll). Cases that depend on PR-3+ behaviour
 * are skipped via {@link #PR2_SCOPE} until their feature lands.
 */
class ConformanceSuiteTest {

    private static final Path CASES_DIR =
            Path.of("..", "..", "conformance", "cases").toAbsolutePath().normalize();

    /**
     * Whitelist of cases this PR is expected to pass. Each subsequent PR
     * extends the set in lockstep with the feature it implements:
     *
     * <ul>
     *   <li>PR-3 adds 05, 06, 11, 12 (renewal, drain, 409, 500-backoff).
     *   <li>PR-4 adds 07, 08, 09, 10 (auth-already-here, self-register, streaming).
     * </ul>
     */
    private static final Set<String> PR2_SCOPE = Set.of(
            "01-poll-empty.yaml", "02-poll-single-success.yaml", "03-handler-failure.yaml", "04-cancel-via-poll.yaml");

    static Stream<Arguments> cases() throws Exception {
        if (!Files.isDirectory(CASES_DIR)) {
            throw new IllegalStateException("Cases dir not found: " + CASES_DIR);
        }
        try (Stream<Path> files = Files.list(CASES_DIR)) {
            return files
                    .filter(p -> p.toString().endsWith(".yaml"))
                    .filter(p -> PR2_SCOPE.contains(p.getFileName().toString()))
                    .sorted()
                    .map(p -> Arguments.of(p.getFileName().toString(), p))
                    .toList()
                    .stream();
        }
    }

    @ParameterizedTest(name = "{0}")
    @MethodSource("cases")
    void conformanceCase(String name, Path caseFile) throws Exception {
        CaseSpec spec = CaseLoader.load(caseFile);
        new ConformanceRunner().run(spec);
    }
}
