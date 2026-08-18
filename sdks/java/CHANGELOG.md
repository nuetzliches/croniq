# Changelog

All notable changes to the Croniq Runner SDK for Java are documented here. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Security

- **HTTPS is required for a non-loopback `serverUrl`
  ([#440](https://github.com/nuetzliches/croniq/issues/440)).** `serverUrl`
  defaulted to `http://localhost:4000` and `URI.create` accepted any `http`
  host silently, so swapping in a real host shipped the API key as a cleartext
  `Authorization` header on every poll, with no warning. `CroniqRunnerOptions`
  and `CroniqClientOptions` now validate the scheme in `Builder.build()` — so
  a misconfiguration fails fast instead of on the first poll. An `https` URL
  is always accepted; `http` only when the host is loopback (`localhost`,
  `127.0.0.0/8`, `::1`), keeping the documented `http://localhost:4000`
  quickstart working; anything else throws `IllegalArgumentException` naming
  the URL and the opt-in. The new `allowInsecureHttp(true)` builder method
  (`croniq.runner.allow-insecure-http` in the Spring Boot starter) accepts a
  cleartext URL deliberately and logs one loud SLF4J warning under
  `io.croniq.runner.config.ServerUrls` instead.

- **`job_key` and `execution_id` no longer reach log messages, and are validated
  on ingest ([#441](https://github.com/nuetzliches/croniq/issues/441)).**
  `ExecutionDispatcher` and `BoundedLogWriter` interpolated both identifiers
  into their SLF4J messages ("Handler for {} threw", "Renew failed for {}: {}"),
  so a server-supplied value carrying CRLF forged log records and one carrying
  ANSI escapes reached the operator's terminal raw. Both now travel as `MDC`
  entries with a constant message — the logging backend owns rendering (add
  `%X{job_key}` / `%X{execution_id}` to a Logback pattern, or use a structured
  encoder), and the SDK does not escape a second time.
  `ExecutionDispatcher.dispatch` additionally validates both identifiers. A
  `job_key` is refused only for containing a control character — C0, DEL or C1
  — or exceeding 256 code points; every printable character in any script is
  accepted, interior spaces included, because
  `job "billing:monthly invoice" { … }` is legal DSL and `POST /v1/jobs`
  constrains the key not at all. Execution ids keep a narrow
  `a-z A-Z 0-9 - _ . :` charset up to 64 characters, which the server's v4 UUIDs
  satisfy strictly. A refused assignment with a *valid* `execution_id` is acked
  as a failure naming the offending field, so it dead-letters rather than
  looping; one whose `execution_id` is itself unsafe is dropped, since nothing
  safely addresses the server. New public helper
  `io.croniq.runner.internal.IdentifierGuard` holds the rules.

- **The conformance harness pins SnakeYAML's `SafeConstructor`
  ([#443](https://github.com/nuetzliches/croniq/issues/443)).** `CaseLoader`
  and `YamlSupport` loaded the shared YAML fixtures through a bare
  `new Yaml()`. Not exploitable as shipped — SnakeYAML 2.x's default
  `TagInspector` rejects global tags (the CVE-2022-1471 fix) and the input is
  repo-local fixtures rather than network data — but that safety is a
  version-dependent default, so both now construct
  `new Yaml(new SafeConstructor(new LoaderOptions()))`, which is not. Test
  harness only; no published artifact changed.

## [0.3.0] - 2026-07-18

### Added — logical fire time

- **`CroniqExecutionContext.scheduledFor()`** exposes the trigger's original
  logical fire time (`java.time.Instant`), stable across retries and dead-letter
  replays. Use it for time-relative job logic (e.g. the month a report covers)
  instead of `Instant.now()`. `null` when the server predates the field — the
  SDK never falls back to the queue fire time.
- **Breaking (source):** the `WorkAssignment` record gains a `scheduledFor`
  component; test doubles that construct it positionally must add the argument.

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
