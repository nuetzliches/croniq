package io.github.nuetzliches.croniq.runner;

import static org.junit.jupiter.api.Assertions.assertNotNull;

import org.junit.jupiter.api.Test;

class RunnerTest {

    @Test
    void versionIsNeverNull() {
        // Smoke test for the placeholder. Real conformance suite lands with
        // the runner implementation per issue #133.
        assertNotNull(Runner.version());
    }
}
