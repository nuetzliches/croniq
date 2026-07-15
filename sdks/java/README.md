# Croniq Runner SDK for Java

[![Maven Central](https://img.shields.io/maven-central/v/io.github.nuetzliches/croniq-runner.svg)](https://central.sonatype.com/artifact/io.github.nuetzliches/croniq-runner)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Build job execution runners for [Croniq](https://github.com/nuetzliches/croniq) in Java (21+) or Kotlin. The SDK polls a Croniq server for work, dispatches typed handlers, streams structured logs back, and reports completion — using virtual threads (Project Loom), `java.net.http.HttpClient`, Jackson, and SLF4J only.

> **Status:** feature-complete as of v0.16.0. All seven PRs of [#133](https://github.com/nuetzliches/croniq/issues/133) have landed — core poll/ack loop, lease renewal, graceful drain, streaming logs, auth header, self-register, Spring Boot starter, Kotlin coroutine extensions, OpenTelemetry observer. First Maven Central publish is queued behind the Sonatype Central Portal sign-off; until that goes through, the artefacts are reachable by building from source (`./gradlew publishToMavenLocal`) or by pulling the snapshot CI job's `maven-local-smoke` artefact. The badge above will start resolving once the first release goes public.

## Modules

| Coordinates                                                | Purpose                                                                       |
| ---------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `io.github.nuetzliches:croniq-runner`                      | Core SDK. JDK 21 + Jackson + SLF4J. No Spring / Kotlin / OTel dependency.     |
| `io.github.nuetzliches:croniq-runner-spring-boot-starter`  | Opt-in Spring Boot 3 auto-config, `@CroniqJob`, `croniq.runner.*` properties. |
| `io.github.nuetzliches:croniq-runner-kotlin-ext`           | Coroutine adapters + Kotlin DSL on top of the Java core.                      |
| `io.github.nuetzliches:croniq-runner-opentelemetry`        | Opt-in OpenTelemetry observer — one span per execution.                       |
| `conformance-tests` (not published)                        | Runs `sdks/conformance/cases/*.yaml` against the SDK.                         |

> **Java packages remain `io.croniq.runner.*`** so imports stay short and natural. Maven Central does not require the Java package to mirror the group ID — only the group ID itself must be under a verified namespace. If/when `io.croniq` gets verified as a separate Sonatype namespace, the group ID can switch over without renaming a single source file.

## Toolchain

- **JDK 21+** required. Virtual threads change the concurrency design enough that supporting earlier JDKs would mean two parallel implementations — see issue #133's "Out of scope".
- **Gradle 8.10+** with the Kotlin DSL. The wrapper script (`./gradlew`) pins the version; no global Gradle install is needed.

## Build & test

```sh
cd sdks/java
./gradlew checkAll        # spotless + checkstyle + test on every module
./gradlew formatAll       # apply spotless formatting in place
./gradlew :core:test      # core unit tests only
./gradlew publishToMavenLocal  # smoke-test publishing to ~/.m2
```

The conformance binding (`conformance-tests` module) gains real cases in PR-2:

```sh
./gradlew :conformance-tests:test
```

## Layout

```
sdks/java/
├── settings.gradle.kts            multi-module declaration
├── build.gradle.kts               root tasks (checkAll, formatAll)
├── gradle/libs.versions.toml      central dependency catalogue
├── buildSrc/                      convention plugins (java/kotlin)
├── config/checkstyle/             style rules
├── core/                          → io.github.nuetzliches:croniq-runner
├── spring-boot-starter/           → io.github.nuetzliches:croniq-runner-spring-boot-starter
├── kotlin-ext/                    → io.github.nuetzliches:croniq-runner-kotlin-ext
└── conformance-tests/             YAML-driven wire-protocol suite
```

This mirrors the reference .NET SDK at [`sdks/dotnet/src/Croniq.Runner.Sdk/`](../dotnet/src/Croniq.Runner.Sdk/) so contributors who know one ecosystem can navigate the other.

## Triggering jobs on demand (producer)

The runner above is the **consumer** side. The **producer** side — firing a job _immediately_, e.g. in response to an application event — is a separate, first-class client (`CroniqTriggerClient`, in the `core` module) that wraps `POST /v1/trigger`. It is independent of `CroniqRunner`: a pure producer never polls, and it carries its **own** credentials (`CroniqClientOptions`), because triggering needs the `jobs:trigger` (or `admin`) scope that runner poll keys typically don't carry. (Parity with the .NET SDK's `ICroniqTriggerClient`.)

```java
import io.croniq.runner.CroniqTriggerClient;
import io.croniq.runner.TriggerRequest;
import io.croniq.runner.TriggerResult;
import io.croniq.runner.config.CroniqClientOptions;
import java.util.List;
import java.util.Map;

// Thread-safe and intended to be long-lived: build one per server + credential and share it.
var client = new CroniqTriggerClient(
    CroniqClientOptions.builder()
        .serverUrl("http://localhost:4000")
        .apiKey(System.getenv("CRONIQ_TRIGGER_KEY")) // jobs:trigger scope — NOT a runner poll key
        .build());

TriggerResult result = client.trigger(
    TriggerRequest.builder("billing:invoice-generate")
        .metadata(Map.of("invoice_id", "inv_42"))
        .require(List.of("billing"))
        .prefer(List.of("eu-central"))
        .timeout("10m")
        .idempotencyKey("evt-2026-07-14-001") // optional server-side dedup
        .build());

// result.executionId(), result.queued(), result.deduplicated()
// Fire with no options at all: client.trigger("billing:invoice-generate");
```

The same registered handler serves both its Croniqfile schedule (safety-net / reconcile floor) and near-real-time, event-driven fires — one execution and observability path, no second code path.

`TriggerRequest.builder(jobKey)` takes the same routing/metadata knobs as a scheduled fire:

| Builder method    | Meaning                                                                                        |
| ----------------- | ---------------------------------------------------------------------------------------------- |
| `jobKey`          | Job to fire, e.g. `billing:invoice-generate` (required).                                       |
| `metadata`        | Arbitrary JSON (`Map<String, Object>`) forwarded to the handler, merged over the job's DSL metadata. |
| `require`         | Capabilities a runner **must** have to be assigned this execution.                             |
| `prefer`          | Capabilities used to prefer runners when several are eligible.                                 |
| `timeout`         | Execution timeout as a duration string (`"30s"`, `"5m"`); server default when omitted.         |
| `idempotencyKey`  | Optional dedup key (≤ 200 chars); repeat triggers with the same key coalesce onto the existing execution. |

…and `trigger(...)` returns a `TriggerResult`:

| Accessor          | Meaning                                                                                        |
| ----------------- | ---------------------------------------------------------------------------------------------- |
| `executionId()`   | The created — or, on a dedup hit, the existing — execution.                                    |
| `queued()`        | Server work-queue depth after the trigger was processed.                                       |
| `deduplicated()`  | `true` when coalesced onto an existing execution via `idempotencyKey`; `false` on servers without idempotency support. |

- **Unset optionals are omitted** from the JSON body (never sent as `null`) — a producer never emits `metadata` / `require` / `prefer` / `timeout` / `idempotency_key` the caller didn't supply.
- **Idempotency.** Pass `idempotencyKey` so at-least-once producers (event redelivery, retries, concurrent publishers) coalesce onto one execution; `result.deduplicated()` is `true` when the server returned an existing execution.
- **Backpressure.** Every failure — a non-2xx response, a transport failure, or a serialisation error — surfaces as `CroniqTriggerException` (never a default/empty result). The per-job queue-overflow `429` is `e.isQueueOverflow()` (`e.statusCode()` carries the raw status; `0` for transport errors), so a batching / retrying producer can back off instead of dropping work:

  ```java
  try {
      client.trigger(TriggerRequest.builder("billing:invoice-generate").build());
  } catch (CroniqTriggerException e) {
      if (e.isQueueOverflow()) {
          // 429: job at its per-job queue-depth cap — back off and retry later
      } else {
          throw e;
      }
  }
  ```

## Wire-protocol conformance

The SDK is validated against the shared, language-neutral conformance suite at [`sdks/conformance/`](../conformance/) — the same 12 YAML cases that drive the .NET SDK. Future Python / Go / TypeScript SDKs will pass the same cases.

When the wire protocol gains a new behaviour, the case is added to `sdks/conformance/cases/` first.

## Compatibility matrix

| SDK Version | Croniq Server (min) | Croniq Server (max tested) | JDK     |
| ----------- | ------------------- | -------------------------- | ------- |
| 0.1.x       | 0.14.0              | 0.14.0                     | 21, 23  |

## License

Dual-licensed under MIT OR Apache-2.0. See [LICENSE-MIT](../../LICENSE-MIT) and [LICENSE-APACHE](../../LICENSE-APACHE) at the repo root.
