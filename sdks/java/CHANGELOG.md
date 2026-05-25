# Changelog

All notable changes to the Croniq Runner SDK for Java are documented here. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Initial Gradle multi-module skeleton (`core`, `spring-boot-starter`,
  `kotlin-ext`, `conformance-tests`). PR-1 of
  [#133](https://github.com/nuetzliches/croniq/issues/133).
- Convention plugins under `buildSrc/` for shared toolchain, Spotless
  (Palantir Java format), Checkstyle, JUnit 5 config.
- Central dependency catalogue at `gradle/libs.versions.toml`.
- CI workflow `.github/workflows/java-sdk-ci.yml` with schema validation,
  matrix build, conformance suite, and Maven-local publish smoke test.
- `CroniqRunner.sdkVersion()` placeholder API surface — replaced by the
  real runner entry point in PR-2.
