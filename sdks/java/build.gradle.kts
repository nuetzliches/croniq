// Root build — shared task wiring across the multi-module project.
// Per-module config lives in each subproject's build.gradle.kts and
// the convention plugins under buildSrc/.

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
