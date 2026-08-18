package io.croniq.runner.conformance;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatCode;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.DisplayName;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.CsvSource;

/**
 * Guards the loaders' own failure mode.
 *
 * <p>The loaders pick keys out of the SnakeYAML map by name, so before #460 a
 * key they did not ask for was dropped without a sound: a case carrying an
 * assertion this binding had not implemented loaded cleanly, was not asserted,
 * and the suite stayed green precisely when the contract stopped being
 * enforced. These tests provoke that silence and assert it is now noisy —
 * the same anti-rot role {@code unsupportedEntriesReferenceExistingCases}
 * plays for the exclusion list (#453).
 */
class CaseLoaderStrictnessTest {

    private static final String MINIMAL_CASE =
            """
            name: strictness probe
            runner_config:
              capabilities: ["work"]
            handlers:
              - job_key: "work:probe"
                behavior: noop
            server_script:
              - on: "POST /v1/work/poll"
                respond:
                  status: 200
                  body: { work: [], cancel: [] }
            expectations:
              duration_max_ms: 500
              http:
                - method: POST
                  path: /v1/work/poll
                  min_count: 1
            """;

    private static final String MINIMAL_TRIGGER_CASE =
            """
            name: strictness probe
            trigger_config:
              api_key: "croniq_testkey"
            trigger_calls:
              - request:
                  job_key: "work:probe"
                expect:
                  response:
                    execution_id: "*"
            server_script:
              - on: "POST /v1/trigger"
                respond:
                  status: 200
                  body: { execution_id: "exec-001", queued: 1, deduplicated: false }
            expectations:
              duration_max_ms: 500
              http:
                - method: POST
                  path: /v1/trigger
                  exact_count: 1
            """;

    @TempDir
    Path tempDir;

    /**
     * One row per level a runner case nests: an unknown key has to be caught at
     * each of them, not merely at the top. The indent column decides <em>which</em>
     * mapping gains the key, so it cannot always be read off the anchor line — a
     * key of a {@code - } list item sits two columns right of the dash.
     */
    @ParameterizedTest(name = "rejects an unknown key in {0}")
    @CsvSource(
            delimiter = '|',
            value = {
                "case                | name: strictness probe        | -1",
                "runner_config       | '  capabilities: [\"work\"]'    | -1",
                "handler             | '    behavior: noop'          | -1",
                "server_script entry | '  - on: \"POST /v1/work/poll\"' | 4",
                "respond             | '      status: 200'           | -1",
                "expectations        | '  duration_max_ms: 500'      | -1",
                "http expectation    | '      min_count: 1'          | -1",
            })
    void load_rejects_a_key_the_binding_does_not_model(String level, String anchor, int indent) throws IOException {
        Path file = write(inject(MINIMAL_CASE, anchor, indent));

        assertThatThrownBy(() -> CaseLoader.load(file))
                .isInstanceOf(IllegalStateException.class)
                .hasMessageContaining("not_a_real_key")
                .hasMessageContaining(level);
    }

    @ParameterizedTest(name = "rejects an unknown key in {0}")
    @CsvSource(
            delimiter = '|',
            value = {
                "trigger case             | name: strictness probe          | -1",
                "trigger_config           | '  api_key: \"croniq_testkey\"'   | -1",
                "trigger_calls request    | '      job_key: \"work:probe\"'   | -1",
                // Same anchor, two indents: dedenting to 6 closes `response:` and
                // adds the key to `expect`; staying at 8 adds it to `response`.
                "trigger_calls expect     | '        execution_id: \"*\"'     | 6",
                "expect.response          | '        execution_id: \"*\"'     | -1",
                "http expectation         | '      exact_count: 1'          | -1",
            })
    void loadTrigger_rejects_a_key_the_binding_does_not_model(String level, String anchor, int indent)
            throws IOException {
        Path file = write(inject(MINIMAL_TRIGGER_CASE, anchor, indent));

        assertThatThrownBy(() -> TriggerCaseLoader.load(file))
                .isInstanceOf(IllegalStateException.class)
                .hasMessageContaining("not_a_real_key")
                .hasMessageContaining(level);
    }

    @Test
    @DisplayName("body_absent is trigger-only: runner cases must not carry it")
    void body_absent_is_trigger_only() throws IOException {
        // Both shapes decode HTTP expectations through the same helper, so
        // without per-shape key sets a runner case could quietly carry a
        // trigger-only key. case-schema.json does not declare body_absent.
        Path runnerCase =
                write(MINIMAL_CASE.replace("      min_count: 1", "      min_count: 1\n      body_absent: [metadata]"));
        assertThatThrownBy(() -> CaseLoader.load(runnerCase))
                .isInstanceOf(IllegalStateException.class)
                .hasMessageContaining("body_absent");

        Path triggerCase = write(MINIMAL_TRIGGER_CASE.replace(
                "      exact_count: 1", "      exact_count: 1\n      body_absent: [metadata]"));
        assertThat(TriggerCaseLoader.load(triggerCase)
                        .expectations()
                        .http()
                        .get(0)
                        .bodyAbsent())
                .containsExactly("metadata");
    }

    @Test
    @DisplayName("strictness must not reject the vocabulary the corpus uses")
    void loaders_accept_the_known_vocabulary() throws IOException {
        // Counterweight to the negative tests: a fixture that failed to load on
        // its own would make every one of them pass for the wrong reason.
        Path runnerCase = write(MINIMAL_CASE);
        assertThatCode(() -> CaseLoader.load(runnerCase)).doesNotThrowAnyException();
        assertThat(CaseLoader.load(runnerCase).handlers()).hasSize(1);

        Path triggerCase = write(MINIMAL_TRIGGER_CASE);
        assertThatCode(() -> TriggerCaseLoader.load(triggerCase)).doesNotThrowAnyException();
        assertThat(TriggerCaseLoader.load(triggerCase).triggerCalls()).hasSize(1);
    }

    /**
     * Insert an unrecognised key after {@code anchor}. A negative {@code indent}
     * means "use the anchor's own indentation".
     */
    private static String inject(String text, String anchor, int indent) {
        assertThat(text).as("fixture must contain the anchor").contains(anchor);
        int column =
                indent >= 0 ? indent : anchor.length() - anchor.stripLeading().length();
        return text.replace(anchor, anchor + "\n" + " ".repeat(column) + "not_a_real_key: 1");
    }

    private Path write(String yaml) throws IOException {
        Path file = tempDir.resolve("case-%d.yaml".formatted(yaml.hashCode() & 0xffff));
        Files.writeString(file, yaml);
        return file;
    }
}
