// io.croniq:runner — the core SDK. Library-not-framework: no Spring or
// Kotlin dependency leaks here. JDK 21 + Jackson + SLF4J only.

plugins {
  id("croniq.java-conventions")
  id("croniq.publish-conventions")
}

description = "Croniq Runner SDK — polls work, dispatches handlers, streams logs."

// Core publishes as `io.croniq:runner`. The Gradle project name is `core`
// for filesystem clarity; renaming the publication keeps the published
// coordinates short and discoverable.
publishing {
  publications.named<MavenPublication>("maven") {
    artifactId = "runner"
  }
}

dependencies {
  // JSON over the wire. Aligned with the .NET SDK's System.Text.Json
  // + source-generated context. `api` because JsonNode appears in the
  // public CroniqExecutionContext.metadata() signature.
  api(libs.jackson.databind)
  implementation(libs.jackson.datatype.jsr310)

  // SLF4J is the only logging surface the SDK exposes. `api` because
  // CroniqExecutionContext.logger() returns org.slf4j.Logger. The
  // starter and examples wire Logback (or Log4j2) downstream. We
  // deliberately do NOT pull a binding into core — that would conflict
  // with consumer logging frameworks.
  api(libs.slf4j.api)

  testImplementation(platform(libs.junit.bom))
  testImplementation(libs.bundles.junit)
  testImplementation(libs.mockito.core)
  testImplementation(libs.mockito.junit.jupiter)
  testImplementation(libs.wiremock)
  testImplementation(libs.awaitility)
  testRuntimeOnly(libs.logback.classic)
}
