package io.croniq.runner.kotlin

import io.croniq.runner.CroniqRunner

/**
 * Kotlin-idiomatic facade over the Java core. Mostly hosts extension functions
 * (see [CoroutineHandlerExtensions]); this object is the parking spot for
 * version metadata and any non-extension helpers.
 */
public object CroniqRunnerKotlin {
    /** Convenience accessor for the Java SDK's version string. */
    public val sdkVersion: String
        get() = CroniqRunner.sdkVersion()
}

/**
 * Top-level convenience entry. Build, run synchronously, drain on exit.
 *
 * ```kotlin
 * croniqRunner {
 *     options(CroniqRunnerOptions.builder().serverUrl("http://localhost:4000").build())
 *     addJob("billing:invoice") { ctx -> /* suspend body */ }
 * }
 * ```
 */
public fun croniqRunner(configure: CroniqRunner.Builder.() -> Unit) {
    val builder = CroniqRunner.builder().apply(configure)
    builder.build().use { it.run() }
}
