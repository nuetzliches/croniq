# Changelog

All notable changes to the Java/Kotlin runner SDK are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the package adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.1] - Unreleased

### Added

- Gradle (Kotlin DSL) project scaffolding under `sdks/java/`.
- Placeholder `io.github.nuetzliches.croniq.runner.Runner` class so the
  `0.0.x` line can publish to Maven Central as a pipeline smoke test.
- `java-sdk-release.yml` workflow — tag-triggered (`java-sdk-vX.Y.Z`),
  builds + signs + publishes to the Sonatype Central Portal via the
  Vanniktech `com.vanniktech.maven.publish` plugin.

### Notes

- The real runner API (polling, dispatch, log streaming, conformance binding,
  Spring Boot starter, Kotlin extensions) is tracked in
  [issue #133](https://github.com/nuetzliches/croniq/issues/133) and will
  land in subsequent PRs against the `0.1.x` line.
