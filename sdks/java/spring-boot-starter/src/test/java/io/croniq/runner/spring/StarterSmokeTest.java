package io.croniq.runner.spring;

import static org.assertj.core.api.Assertions.assertThat;

import io.croniq.runner.CroniqRunner;
import org.junit.jupiter.api.Test;

class StarterSmokeTest {

    @Test
    void starterCanSeeCoreModule() {
        // Verifies the module graph: the starter must transitively expose the
        // core SDK's public types to its tests (and to downstream consumers).
        assertThat(CroniqRunner.sdkVersion()).isNotBlank();
    }
}
