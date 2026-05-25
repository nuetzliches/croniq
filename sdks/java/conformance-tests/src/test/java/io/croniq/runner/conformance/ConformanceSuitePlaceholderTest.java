package io.croniq.runner.conformance;

import static org.assertj.core.api.Assertions.assertThat;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.stream.Stream;
import org.junit.jupiter.api.Test;

/**
 * Placeholder for the conformance binding. The real harness — YAML case
 * loader, WireMock server, sentinel handlers, expectation assertions —
 * lands in PR-2 alongside the first protocol implementation.
 *
 * <p>For now this test just asserts that the YAML cases exist at the
 * expected path. That single assertion is enough to wire the module into
 * the CI workflow and exercise its Gradle config end-to-end.
 */
class ConformanceSuitePlaceholderTest {

    private static final Path CASES_DIR =
            Path.of("..", "..", "conformance", "cases").toAbsolutePath().normalize();

    @Test
    void conformanceCasesDirectoryIsDiscoverable() throws Exception {
        assertThat(CASES_DIR)
                .as("expected sdks/conformance/cases/ relative to module dir")
                .exists();
        try (Stream<Path> yamls = Files.list(CASES_DIR)) {
            long count = yamls.filter(p -> p.toString().endsWith(".yaml")).count();
            assertThat(count)
                    .as("at least one conformance case YAML must exist")
                    .isGreaterThan(0);
        }
    }
}
