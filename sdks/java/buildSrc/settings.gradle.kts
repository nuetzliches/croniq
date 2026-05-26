// buildSrc is its own composite Gradle build. We point its version catalog
// at the main project's libs.versions.toml so plugin versions stay aligned.
dependencyResolutionManagement {
    versionCatalogs {
        create("libs") {
            from(files("../gradle/libs.versions.toml"))
        }
    }
}
