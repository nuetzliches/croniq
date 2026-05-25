// Croniq Runner SDK for Java — multi-module Gradle build.
//
// One published artifact per included module (apart from conformance-tests,
// which is test-only and never published). Convention plugins in buildSrc
// hold the shared toolchain / formatter / publishing config — see
// buildSrc/src/main/kotlin/.

rootProject.name = "croniq-runner-java"

pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        mavenCentral()
    }
}

include(
    "core",
    "spring-boot-starter",
    "kotlin-ext",
    "otel",
    "conformance-tests",
)
