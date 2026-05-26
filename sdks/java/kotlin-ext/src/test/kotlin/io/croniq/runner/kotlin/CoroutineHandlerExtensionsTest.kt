package io.croniq.runner.kotlin

import io.croniq.runner.CroniqRunner
import io.croniq.runner.config.CroniqRunnerOptions
import io.croniq.runner.handler.CroniqExecutionContext
import org.assertj.core.api.Assertions.assertThat
import org.junit.jupiter.api.Test

class CoroutineHandlerExtensionsTest {
    @Test
    fun `suspend handler registers without exploding`() {
        val builder =
            CroniqRunner
                .builder()
                .options(CroniqRunnerOptions.builder().serverUrl("http://localhost:1").build())

        // Smoke check — the extension function compiles, the type inference
        // picks the suspend overload, and the Builder fluently chains.
        builder.addJob("test:job") { ctx: CroniqExecutionContext ->
            // Reference ctx so the lambda has the right shape and unused-
            // parameter checks pass.
            assertThat(ctx.executionId()).isNotNull()
        }

        builder.addJob("test:scheduled", schedule = "5m") { ctx ->
            assertThat(ctx.jobKey()).isEqualTo("test:scheduled")
        }
        // No assertion on the runner itself — the integration is exercised by
        // the conformance binding once cases that exercise Kotlin handlers
        // land. This test just guards the kotlin-ext public surface.
    }

    @Test
    fun `croniqRunner dsl wires options and jobs`() {
        // The DSL's run() would block forever against a real server. We don't
        // invoke it — we just confirm the builder receiver compiles end-to-end.
        val configure: CroniqRunner.Builder.() -> Unit = {
            options(CroniqRunnerOptions.builder().serverUrl("http://localhost:1").build())
            addJob("dsl:noop") { /* suspend body */ }
        }
        val builder = CroniqRunner.builder().apply(configure)
        assertThat(builder).isNotNull
    }
}
