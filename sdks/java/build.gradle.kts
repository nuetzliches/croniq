// Root build — shared task wiring only.
//
// Maven Central publishing is configured per-module via the
// `croniq.publish-conventions` convention plugin (see
// `buildSrc/src/main/kotlin/croniq.publish-conventions.gradle.kts`), which
// applies the Vanniktech maven-publish plugin to each published subproject.
// That plugin handles bundle creation, GPG signing, Sonatype Central Portal
// upload, and auto-release — no root-level publishing config is needed.
//
// (The previous `nexusPublishing { ... }` block was a stale leftover from
// the legacy OSSRH approach. It targeted s01.oss.sonatype.org, which
// rejects io.github.nuetzliches with HTTP 402 because the namespace was
// verified after 2024-06 and exists only on the new Central Portal.)

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

// To publish to Maven Central from a release runner:
//   ./gradlew publishAndReleaseToMavenCentral
//
// The Vanniktech plugin signs, bundles, and uploads to the new Central
// Portal (https://central.sonatype.com) and auto-releases on successful
// validation. PR builds run with empty credentials and skip the publish-
// to-Central path while still exercising publishToMavenLocal as a smoke
// check (see the `publish-smoke` job in the Java SDK CI workflow).
