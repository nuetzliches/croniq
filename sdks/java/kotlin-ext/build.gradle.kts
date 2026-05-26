// io.github.nuetzliches:croniq-runner-kotlin-ext — coroutine adapters and
// Kotlin-idiomatic DSL on top of the Java core. Pure Kotlin; consumers of
// :core in Java don't pull this module.

plugins {
  id("croniq.kotlin-conventions")
  id("croniq.publish-conventions")
}

description = "Kotlin coroutine extensions for the Croniq Runner SDK."

mavenPublishing {
  coordinates(project.group.toString(), "croniq-runner-kotlin-ext", project.version.toString())
}

dependencies {
  api(project(":core"))
  api(libs.kotlinx.coroutines.core)
  // Bridges CompletableFuture <-> coroutine — we use this to adapt the
  // core SDK's CompletionStage-returning handler signature to suspend
  // functions in PR-6.
  implementation(libs.kotlinx.coroutines.jdk8)

  testImplementation(platform(libs.junit.bom))
  testImplementation(libs.bundles.junit)
}
