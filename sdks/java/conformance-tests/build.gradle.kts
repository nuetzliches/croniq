// Test-only module. Loads YAML cases from ../../conformance/cases/,
// stands up WireMock, configures the core SDK against the mock, and
// asserts the wire-level expectations.
//
// Mirrors sdks/dotnet/tests/Croniq.Runner.Sdk.Conformance.Tests/ — the
// same YAML cases drive both bindings.

import java.time.Duration

plugins {
    id("croniq.java-conventions")
}

description = "Conformance binding — runs sdks/conformance/cases/*.yaml against the Java SDK."

// Never published. The Java conventions plugin enables sourcesJar /
// javadocJar by default; turn them off here so `./gradlew assemble`
// doesn't produce artefacts we don't want on Maven Central.
tasks.named<Jar>("sourcesJar") { enabled = false }
tasks.named<Jar>("javadocJar") { enabled = false }

dependencies {
    testImplementation(project(":core"))
    testImplementation(platform(libs.junit.bom))
    testImplementation(libs.bundles.junit)
    testImplementation(libs.wiremock)
    testImplementation(libs.snakeyaml)
    testImplementation(libs.jackson.databind)
    testImplementation(libs.awaitility)
    testRuntimeOnly(libs.logback.classic)
}

tasks.named<Test>("test") {
    // Each case stands up its own WireMock instance on a fresh port; the
    // SDK polls in tight loops. Parallel execution would tangle the
    // recorded request expectations across cases. Force single-threaded.
    systemProperty("junit.jupiter.execution.parallel.enabled", "false")
    // Wall-clock cap: each case declares duration_max_ms (usually 3s);
    // the harness enforces it per-case. 90s is the overall test-task
    // cap so a hung case can't stall CI indefinitely.
    timeout.set(Duration.ofSeconds(90))
}
