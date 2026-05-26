// io.croniq:runner-opentelemetry — opt-in OpenTelemetry instrumentation
// implementing CroniqRunnerObserver. Consumers who don't use OTel pull only
// :core and avoid this artifact entirely.

plugins {
  id("croniq.java-conventions")
  id("croniq.publish-conventions")
}

description = "OpenTelemetry instrumentation for the Croniq Runner SDK."

// Vanniktech maven-publish (applied via croniq.publish-conventions) creates
// its own `maven` publication lazily; the legacy `publications.named` form
// here used to belong to the OSSRH plugin and is no longer wired. Match the
// idiom used by the other published modules.
mavenPublishing {
  coordinates(project.group.toString(), "runner-opentelemetry", project.version.toString())
}

dependencies {
  api(project(":core"))
  // The OpenTelemetry API jar is small and a no-op when no SDK is
  // registered — safe to expose as `api` so users get the OTel types
  // (`OpenTelemetry`, `Tracer`) on their compile classpath when they
  // depend on this module.
  api(libs.opentelemetry.api)

  testImplementation(platform(libs.junit.bom))
  testImplementation(libs.bundles.junit)
  testImplementation(libs.opentelemetry.sdk)
  // opentelemetry-sdk-testing supplies InMemorySpanExporter; pinned via the
  // same `opentelemetry` version ref in libs.versions.toml.
  testImplementation("io.opentelemetry:opentelemetry-sdk-testing:${libs.versions.opentelemetry.get()}")
}
