package io.croniq.runner;

import static org.assertj.core.api.Assertions.assertThat;

import org.junit.jupiter.api.Test;

class CroniqRunnerSmokeTest {

    @Test
    void sdkVersionIsNonEmpty() {
        // Sanity check that the skeleton compiles, JUnit 5 discovers tests,
        // and the Jar manifest fallback is wired. Replaced by real behavioural
        // tests in PR-2.
        assertThat(CroniqRunner.sdkVersion()).isNotBlank();
    }
}
