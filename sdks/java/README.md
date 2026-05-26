# Croniq Runner SDK for Java / Kotlin

[![Maven Central](https://img.shields.io/maven-central/v/io.github.nuetzliches/croniq-runner.svg)](https://central.sonatype.com/artifact/io.github.nuetzliches/croniq-runner)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

> **Status:** scaffolding only. The real runner implementation is tracked in [issue #133](https://github.com/nuetzliches/croniq/issues/133). The current `0.0.x` line exists to validate the publish pipeline end-to-end (build → sign → Maven Central). Do not depend on the placeholder API.

## Install

### Gradle (Kotlin DSL)

```kotlin
dependencies {
    implementation("io.github.nuetzliches:croniq-runner:0.0.1")
}
```

### Maven

```xml
<dependency>
    <groupId>io.github.nuetzliches</groupId>
    <artifactId>croniq-runner</artifactId>
    <version>0.0.1</version>
</dependency>
```

Requires **JDK 21+** (virtual threads).

## Local development

```sh
cd sdks/java
# One-time: generate the Gradle wrapper. Requires a system Gradle 8.11+.
gradle wrapper --gradle-version 8.11

./gradlew build           # compile + test
./gradlew publishToMavenLocal  # install to ~/.m2 for local consumption
```

The release workflow (`.github/workflows/java-sdk-release.yml`) uses Gradle directly via `gradle/actions/setup-gradle`, so committing the wrapper is optional. For local dev ergonomics it is still recommended.

## Release

Cut a release by pushing a tag matching `java-sdk-vX.Y.Z`:

```sh
git tag java-sdk-v0.0.1
git push origin java-sdk-v0.0.1
```

The workflow extracts the version from the tag, builds, signs with the in-memory GPG key, and pushes to the Sonatype Central Portal with `automaticRelease=true` — no manual "Release" click in the portal UI.

## License

Dual-licensed under [Apache-2.0](../../LICENSE-APACHE) OR [MIT](../../LICENSE-MIT) at your option.
