// Kotlin module config — applied only by kotlin-ext. Inherits the Java
// conventions (toolchain, spotless, test config) and layers Kotlin-specific
// settings on top.

plugins {
    id("croniq.java-conventions")
    id("org.jetbrains.kotlin.jvm")
}

kotlin {
    jvmToolchain(21)
    explicitApi() // require explicit visibility modifiers on public API
}

tasks.withType<org.jetbrains.kotlin.gradle.tasks.KotlinCompile>().configureEach {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_21)
        // Treat warnings as errors to keep the Kotlin surface clean.
        allWarningsAsErrors.set(true)
        // -Xjsr305=strict — treat @Nullable/@Nonnull annotations from
        // Java deps as load-bearing for Kotlin's null-safety. The core
        // module uses Jackson which exposes such annotations.
        freeCompilerArgs.add("-Xjsr305=strict")
    }
}

spotless {
    kotlin {
        target("src/**/*.kt")
        ktlint("1.4.1")
        trimTrailingWhitespace()
        endWithNewline()
    }
}
