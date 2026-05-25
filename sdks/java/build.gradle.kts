// Root build — shared task wiring + Sonatype OSSRH staging for Maven Central.
// Per-module config lives in each subproject's build.gradle.kts and
// the convention plugins under buildSrc/.

plugins {
    alias(libs.plugins.nexus.publish)
}

allprojects {
    // Group ID matches the Sonatype-verified namespace
    // (io.github.nuetzliches). See gradle.properties for the migration
    // note about a potential future switch to io.croniq.
    group = "io.github.nuetzliches"
}

// A repo-wide `check` aggregate that runs spotless + checkstyle + tests
// for every module. Mirrors the .NET SDK's `dotnet test Croniq.Runner.Sdk.slnx`
// entry point — one command to validate everything locally before pushing.
tasks.register("checkAll") {
    group = "verification"
    description = "Runs spotlessCheck, checkstyleMain, and test on every module."
    dependsOn(subprojects.map { "${it.path}:check" })
}

tasks.register("formatAll") {
    group = "formatting"
    description = "Applies spotless to every module (writes changes)."
    dependsOn(subprojects.map { "${it.path}:spotlessApply" })
}

// Sonatype OSSRH staging. Credentials are read from environment variables
// (OSSRH_USERNAME / OSSRH_PASSWORD) or `~/.gradle/gradle.properties` — never
// committed. CI sets them from GitHub Actions secrets in PR-7 (this PR).
//
// To publish:
//   ./gradlew publishToSonatype closeAndReleaseSonatypeStagingRepository
//
// Snapshots (-SNAPSHOT versions) auto-promote to s01.oss.sonatype.org's
// snapshots repository; releases stage and require manual closeAndRelease.
nexusPublishing {
    repositories {
        sonatype {
            // s01 is the modern OSSRH endpoint — accounts registered after
            // 2021 must use this URL. Older io.croniq registrations would
            // use https://oss.sonatype.org/.
            nexusUrl.set(uri("https://s01.oss.sonatype.org/service/local/"))
            snapshotRepositoryUrl.set(uri("https://s01.oss.sonatype.org/content/repositories/snapshots/"))
            username.set(providers.environmentVariable("OSSRH_USERNAME").orElse(""))
            password.set(providers.environmentVariable("OSSRH_PASSWORD").orElse(""))
        }
    }
}
