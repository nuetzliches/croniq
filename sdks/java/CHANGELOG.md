# Changelog

All notable changes to the Croniq Runner SDK for Java are documented here. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added — PR-3 of [#133](https://github.com/nuetzliches/croniq/issues/133)

- Lease renewal: each in-flight execution gets a virtual-thread renewer
  that calls `POST /v1/work/renew` at `renewInterval`. Renewer is
  interrupted when the handler returns or fails.
- Graceful drain in `CroniqRunner.close()`: stops the poll loop, waits up
  to `drainTimeout` for in-flight handlers to complete naturally, then
  force-cancels any stragglers. Mirrors the .NET SDK's `StopAsync` and the
  Rust SDK's drain semantics — host shutdown stops new work but does NOT
  cancel running handlers.
- Conformance cases 05 (lease renewal), 06 (drain on shutdown),
  11 (poll 409 conflict), 12 (poll 500 backoff) pass against the Java SDK.

### Added — PR-2 of [#133](https://github.com/nuetzliches/croniq/issues/133)

- Wire-protocol DTOs in `io.croniq.runner.protocol`: `PollRequest`,
  `PollResponse`, `WorkAssignment`, `AckRequest`, `RenewRequest`, `WorkEvent`,
  `RegisterJobRequest`. snake_case JSON via `@JsonProperty`.
- Public handler API in `io.croniq.runner.handler`: `CroniqJobHandler`,
  `CroniqExecutionContext`, `CroniqCancellation`, `CroniqHandlerException`.
- `io.croniq.runner.config.CroniqRunnerOptions` with builder, defaults
  mirroring the .NET SDK's `Croniq:Runner` section.
- `CroniqRunner` entry point — virtual-thread executor per execution,
  single-threaded poll loop, `ApiKey` / `Bearer` auth, server-initiated
  cancellation via `PollResponse.cancel`.
- Internal transport (`CroniqClient`) over `java.net.http.HttpClient`,
  persistent runner-id resolver, humane duration parser.
- Conformance binding in `conformance-tests/` — `CaseLoader` (SnakeYAML),
  in-process `MockServerHarness` (JDK `HttpServer`), `BodyMatcher`,
  `HandlerSentinels`, parameterized JUnit driver. Conformance cases 01-04
  pass against the Java SDK.

### Added — PR-1

- Initial Gradle multi-module skeleton (`core`, `spring-boot-starter`,
  `kotlin-ext`, `conformance-tests`).
- Convention plugins under `buildSrc/` for shared toolchain, Spotless
  (Palantir Java format), Checkstyle, JUnit 5 config.
- Central dependency catalogue at `gradle/libs.versions.toml`.
- CI workflow `.github/workflows/java-sdk-ci.yml` with schema validation,
  matrix build, conformance suite, and Maven-local publish smoke test.
- Release workflow `.github/workflows/java-sdk-release.yml` — tag-triggered
  (`java-sdk-vX.Y.Z`), builds + signs + publishes all three modules to the
  Sonatype Central Portal via Vanniktech `com.vanniktech.maven.publish`
  with `automaticRelease=true` (no manual portal click).
- `CroniqRunner.sdkVersion()` placeholder API surface — replaced by the
  real runner entry point in PR-2.

### Coordinates

- Group ID: `io.github.nuetzliches` (Sonatype-verified namespace). The
  Java packages remain `io.croniq.runner.*` for clean imports — Maven
  Central does not require package-to-group alignment.
- Artifacts: `croniq-runner` (core), `croniq-runner-spring-boot-starter`,
  `croniq-runner-kotlin-ext`. The `croniq-` prefix disambiguates from
  unrelated projects under the same `io.github.nuetzliches` namespace.

### Notes

- The `0.0.x` line is reserved for pipeline smoke tests — published
  artefacts on Maven Central are immutable, so consumers should wait for
  `0.1.0` (PR-2 of #133) before depending on the SDK.
