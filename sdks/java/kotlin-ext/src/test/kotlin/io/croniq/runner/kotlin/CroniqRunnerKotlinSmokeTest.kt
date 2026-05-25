package io.croniq.runner.kotlin

import org.assertj.core.api.Assertions.assertThat
import org.junit.jupiter.api.Test

class CroniqRunnerKotlinSmokeTest {

    @Test
    fun `kotlin facade exposes core sdk version`() {
        // Proves the Kotlin compile/test path works against the Java core.
        // Replaced by coroutine-adapter tests in PR-6.
        assertThat(CroniqRunnerKotlin.sdkVersion).isNotBlank
    }
}
