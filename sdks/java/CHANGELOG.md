# Changelog

All notable changes to the Croniq Runner SDK for Java are documented here. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added — trigger (producer) client, [#285](https://github.com/nuetzliches/croniq/issues/285)

- `io.croniq.runner.CroniqTriggerClient` — producer-side client wrapping
  `POST /v1/trigger`, at parity with the .NET SDK's `ICroniqTriggerClient`
  ([#277](https://github.com/nuetzliches/croniq/issues/277)). Independent of
  `CroniqRunner`: a pure producer needs no runner.
  - `trigger(String jobKey)` and `trigger(TriggerRequest)` return
    `TriggerResult { executionId, queued, deduplicated }`.
  - `TriggerRequest` (builder) carries `jobKey` plus optional `metadata`
    (arbitrary JSON, not just strings), `require` / `prefer` routing hints,
    `timeout`, and `idempotencyKey`. Unset optionals are omitted from the JSON
    body (never sent as `null`).
  - `deduplicated` defaults to `false` when the server omits it (older builds),
    so the idempotency-key dedup flag ([#279](https://github.com/nuetzliches/croniq/issues/279))
    parses forward-compatibly.
- `io.croniq.runner.config.CroniqClientOptions` — trigger-client config with its
  own credentials (`apiKey` / `bearerToken`, `serverUrl`, `requestTimeout`). The
  endpoint needs the `jobs:trigger` (or `admin`) scope, distinct from the runner's
  poll scopes, so the trigger client never reuses the runner's key.
- `io.croniq.runner.CroniqTriggerException` — thrown on a non-2xx response,
  transport failure, or serialisation error. `statusCode()` exposes the HTTP
  status; `isQueueOverflow()` flags the per-job queue-overflow `429`
  ([#299](https://github.com/nuetzliches/croniq/issues/299)) so a batching
  producer can back off rather than drop work.
- Conformance: the binding now runs the shared trigger (producer) cases
  ([#287](https://github.com/nuetzliches/croniq/issues/287)) via
  `TriggerConformanceSuiteTest`. The suite no-ops gracefully until
  `sdks/conformance/cases-trigger/` is present in the checkout.

### Added — PR-7 of [#133](https://github.com/nuetzliches/croniq/issues/133)

- `io.croniq.runner.handler.CroniqRunnerObserver` public interface — lifecycle
  hook for downstream observability (tracing, metrics, audit). Register via
  `CroniqRunner.Builder.observer(...)`. Observers receive `onExecutionStart`
  and `onExecutionEnd` callbacks; exceptions they raise are logged and
  swallowed so observability never blocks job dispatch.
- New `otel/` module → `io.croniq:runner-opentelemetry`:
  - `OpenTelemetryObserver` emits one OTel span per execution. Span name
    `croniq.execute <job_key>`; attributes mirror the .NET SDK
    (`croniq.job.key`, `croniq.execution.id`, `croniq.execution.attempt`,
    `croniq.runner.id`, `croniq.execution.outcome`).
  - Constructor takes either an `OpenTelemetry` instance or a pre-built
    `Tracer`. Span lookup by `execution_id` via `ConcurrentHashMap` so the
    start / end callbacks can run on different virtual threads safely.
- Maven Central publishing wired in:
  - `gradle-nexus-publish-plugin` configured for Sonatype OSSRH s01 staging.
  - GPG signing in `croniq.publish-conventions` — only active when
    `GPG_SIGNING_KEY` is in the environment, so local
    `publishToMavenLocal` runs need no key setup.
  - Credentials read from env vars (`OSSRH_USERNAME`, `OSSRH_PASSWORD`,
    `GPG_SIGNING_KEY`, `GPG_SIGNING_PASSWORD`) so CI plugs them in from
    GitHub Actions secrets.
- README quick-starts for plain Java, Spring Boot, Kotlin coroutines, and
  OpenTelemetry usage.

### Added — PR-6 of [#133](https://github.com/nuetzliches/croniq/issues/133)

- `io.croniq:runner-kotlin-ext` is now functional:
  - Suspend-handler overloads of `CroniqRunner.Builder.addJob()` — extension
    functions in Kotlin take `suspend (CroniqExecutionContext) -> Unit`.
  - `runBlocking(Dispatchers.IO)` bridge: handlers run on the SDK's virtual
    thread, suspend calls inside use the IO dispatcher.
  - Cancellation bridge: a watcher coroutine polls the server-side
    `CroniqCancellation` flag (50ms cadence) and cancels the runBlocking
    root job, so suspending calls unwind via `CancellationException`.
  - `croniqRunner { … }` top-level DSL — build, run, drain on exit.

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
