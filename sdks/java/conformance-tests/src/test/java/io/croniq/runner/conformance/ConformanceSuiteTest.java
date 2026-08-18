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
 * Drives every YAML case under {@code sdks/conformance/cases/} against the
 * Java SDK. Each case becomes one JUnit test so the IDE / CI surfaces it by
 * name when it fails.
 *
 * <p><strong>Every case in the corpus runs by default.</strong> A new YAML
 * dropped into {@code sdks/conformance/cases/} is picked up automatically and
 * has to pass — there is no allowlist to forget to update. This suite used to
 * filter the corpus through a hardcoded {@code SCOPE} set, which meant an
 * unlisted case was silently not run: no skip reported, suite green, CI green.
 * That trap fired twice (cases 13/14 in #452, case 15 in #458) and issue #453
 * inverted the default.
 *
 * <p>The only escape hatch is {@link #UNSUPPORTED}, and it is deliberately
 * expensive to use: an exclusion needs a written reason, it shows up in the
 * test report as a skipped test carrying that reason, and
 * {@link #unsupportedEntriesReferenceExistingCases()} fails the suite if the
 * entry names a file that is no longer in the corpus. The list therefore
 * cannot rot into silence the way {@code SCOPE} did.
 */
class ConformanceSuiteTest {

    private static final Path CASES_DIR =
            Path.of("..", "..", "conformance", "cases").toAbsolutePath().normalize();

    /**
     * Cases the Java SDK deliberately does not run, mapped to the reason why.
     *
     * <p>This is an exclusion list, not an inclusion list: anything absent from
     * it runs. Add an entry only when a case is genuinely not applicable to the
     * Java binding (a protocol feature the Java SDK does not expose, a harness
     * capability the JVM cannot provide) — never to park a failing case. An
     * excluded case is reported as skipped with its reason attached, so an
     * operator reading CI output can see what did not run and why.
     *
     * <p>Empty as of #453: the Java SDK is feature-complete against the corpus
     * and runs all of it.
     */
    private static final Map<String, String> UNSUPPORTED = Map.of();

    /** Every {@code *.yaml} in the corpus, sorted, regardless of support status. */
    private static List<Path> corpus() throws IOException {
        if (!Files.isDirectory(CASES_DIR)) {
            throw new IllegalStateException("Cases dir not found: " + CASES_DIR);
        }
        try (Stream<Path> files = Files.list(CASES_DIR)) {
            return files.filter(p -> p.toString().endsWith(".yaml")).sorted().toList();
        }
    }

    static Stream<Arguments> cases() throws Exception {
        List<Path> corpus = corpus();
        // A mistyped or moved corpus path would otherwise leave this suite
        // passing vacuously with zero tests, which is the exact failure mode
        // #453 is about. Fail loudly instead.
        if (corpus.isEmpty()) {
            throw new IllegalStateException("No conformance cases found under " + CASES_DIR);
        }
        return corpus.stream().map(p -> Arguments.of(label(p), p)).toList().stream();
    }

    /**
     * Display name for one case. An excluded case carries its reason in the
     * test name itself, not only in the abort message: Gradle's JUnit XML
     * writer emits a bare {@code <skipped/>} element and drops the message, so
     * a reason passed only to {@link Assumptions#abort(String)} would be
     * invisible to anything reading the XML. Putting it in the name means the
     * exclusion and its justification survive into every report format.
     */
    private static String label(Path caseFile) {
        String name = caseFile.getFileName().toString();
        String reason = UNSUPPORTED.get(name);
        return reason == null ? name : name + " [SKIPPED: " + reason + "]";
    }

    /**
     * Guards the exclusion list against rot. An {@link #UNSUPPORTED} entry that
     * names a file no longer in the corpus is a stale exclusion — the case may
     * have been renamed, in which case the rename silently re-enabled nothing
     * and the reason now documents a file that does not exist.
     */
    @Test
    void unsupportedEntriesReferenceExistingCases() throws Exception {
        Set<String> present = new TreeSet<>();
        for (Path p : corpus()) {
            present.add(p.getFileName().toString());
        }
        assertThat(present).as("conformance corpus under %s", CASES_DIR).isNotEmpty();
        assertThat(UNSUPPORTED.keySet())
                .as("every UNSUPPORTED entry must name a case that exists in %s", CASES_DIR)
                .allSatisfy(name -> assertThat(present).contains(name));
    }

    @ParameterizedTest(name = "{0}")
    @MethodSource("cases")
    void conformanceCase(String label, Path caseFile) throws Exception {
        String reason = UNSUPPORTED.get(caseFile.getFileName().toString());
        if (reason != null) {
            // Assumptions.abort marks the test skipped *with the reason visible*
            // in the JUnit report, rather than dropping it from the run entirely.
            Assumptions.abort("Not supported by the Java SDK: " + reason);
        }

        CaseSpec spec = CaseLoader.load(caseFile);
        new ConformanceRunner().run(spec);
    }
}
