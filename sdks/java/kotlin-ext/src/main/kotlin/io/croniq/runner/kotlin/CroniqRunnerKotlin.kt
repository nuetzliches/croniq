package io.croniq.runner.kotlin

import io.croniq.runner.CroniqRunner

/**
 * Kotlin-idiomatic facade over the Java core. Coroutine adapters and the
 * `@CroniqJob`-equivalent DSL land in PR-6 of issue #133.
 *
 * Kept as a thin top-level for now so the kotlin-ext module's source root,
 * compile config, and ktlint integration are exercised by the smoke test.
 */
public object CroniqRunnerKotlin {

    /** Convenience accessor for the Java SDK's version string. */
    public val sdkVersion: String
        get() = CroniqRunner.sdkVersion()
}
