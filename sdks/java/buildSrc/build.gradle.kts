plugins {
    `kotlin-dsl`
}

repositories {
    gradlePluginPortal()
    mavenCentral()
}

// Plugin classpath dependencies for the convention plugins under
// src/main/kotlin. Versions duplicated from gradle/libs.versions.toml —
// the LibrariesForLibs accessor isn't generated for buildSrc/build.gradle.kts
// itself (Gradle limitation), so bumps need to happen in both places.
// CI's dependency-versions job will fail loudly when they drift.
dependencies {
    implementation("org.jetbrains.kotlin:kotlin-gradle-plugin:2.0.21")
    implementation("com.diffplug.spotless:spotless-plugin-gradle:6.25.0")
    // Sonatype Central Portal upload + GPG signing. See
    // gradle/libs.versions.toml for the rationale on choosing this over
    // io.github.gradle-nexus.publish-plugin.
    implementation("com.vanniktech:gradle-maven-publish-plugin:0.30.0")
}
