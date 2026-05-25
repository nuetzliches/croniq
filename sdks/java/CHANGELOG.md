# Changelog

All notable changes to the Croniq Runner SDK for Java are documented here. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added — PR-5 of [#133](https://github.com/nuetzliches/croniq/issues/133)

- `io.croniq:runner-spring-boot-starter` is now functional:
  - `@CroniqJob(key=..., schedule=...)` annotation for marking handler methods.
  - `@ConfigurationProperties("croniq.runner")` binding via `CroniqProperties`.
  - `CroniqRunnerAutoConfiguration` with `@ConditionalOnMissingBean` so apps
    can supply their own runner/options for testing.
  - `CroniqRunnerLifecycle` (Spring `SmartLifecycle`) — late start, early
    stop so the runner drains before data sources / brokers tear down.
  - Auto-config metadata file at
    `META-INF/spring/org.springframework.boot.autoconfigure.AutoConfiguration.imports`.
- `croniq.runner.enabled=false` disables the auto-configured runner.

### Added — PR-4 of [#133](https://github.com/nuetzliches/croniq/issues/133)

- `io.croniq.runner.handler.CroniqLogWriter` public interface — streaming
  structured-log sink available on every `CroniqExecutionContext`.
- `BoundedLogWriter` implementation: virtual-thread flusher, batched POSTs
  to `/v1/work/{execution_id}/events` (32 events per batch), time-based
  flush every `renewInterval`, drain-before-ack guarantee.
- `CroniqRunner.Builder.addJob(jobKey, schedule, handler)` overload —
  scheduled handlers self-register via `POST /v1/jobs/register` at runner
  startup. Best-effort; failures are logged but don't block the poll loop
  (registration is idempotent on the server side).
- Conformance cases 07 (ApiKey header), 08 (self-register), 09 (drain-before-ack),
  10 (time-threshold flush) now pass — the Java SDK passes all 12 conformance
  cases the wire protocol describes.

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
