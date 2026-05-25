# Croniq Runner SDK for Java

[![Maven Central](https://img.shields.io/maven-central/v/io.croniq/runner.svg)](https://central.sonatype.com/artifact/io.croniq/runner)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Build job execution runners for [Croniq](https://github.com/nuetzliches/croniq) in Java (21+) or Kotlin. The SDK polls a Croniq server for work, dispatches typed handlers, streams structured logs back, and reports completion — using virtual threads (Project Loom), `java.net.http.HttpClient`, Jackson, and SLF4J only.

The SDK passes all 12 cases in the shared, language-neutral [conformance suite](../conformance/) — wire-protocol parity with the reference .NET SDK.

## Modules

| Coordinates                            | Purpose                                                                       |
| -------------------------------------- | ----------------------------------------------------------------------------- |
| `io.croniq:runner`                     | Core SDK. JDK 21 + Jackson + SLF4J. No Spring / Kotlin / OTel dependency.     |
| `io.croniq:runner-spring-boot-starter` | Opt-in Spring Boot 3 auto-config, `@CroniqJob`, `croniq.runner.*` properties. |
| `io.croniq:runner-kotlin-ext`          | Coroutine adapters + Kotlin DSL on top of the Java core.                      |
| `io.croniq:runner-opentelemetry`       | Opt-in OpenTelemetry observer — one span per execution.                       |
| `conformance-tests` (not published)    | Runs `sdks/conformance/cases/*.yaml` against the SDK.                         |

## Quick start (plain Java)

```java
import io.croniq.runner.CroniqRunner;
import io.croniq.runner.config.CroniqRunnerOptions;

public class Main {
    public static void main(String[] args) throws InterruptedException {
        var options = CroniqRunnerOptions.builder()
                .serverUrl("http://localhost:4000")
                .apiKey(System.getenv("CRONIQ_API_KEY"))
                .capabilities(java.util.List.of("billing"))
                .build();

        try (var runner = CroniqRunner.builder()
                .options(options)
                .addJob("billing:invoice", ctx -> {
                    ctx.logger().info("Hello from {} (attempt {})", ctx.jobKey(), ctx.attempt());
                    ctx.logWriter().write("info", "Processing customer " + ctx.metadata().get("customer_id"));
                })
                .build()) {
            runner.run(); // blocks until close() is called from another thread
        }
    }
}
```

## Spring Boot

```java
@Component
public class BillingJobs {
    @CroniqJob(key = "billing:invoice", schedule = "5m")
    public void handleInvoice(CroniqExecutionContext ctx) {
        // ...
    }
}
```

```yaml
# application.yml
croniq:
  runner:
    server-url: https://croniq.internal
    api-key: ${CRONIQ_API_KEY}
    capabilities: [billing, reporting]
    tags: ["env=prod", "lang=java"]
```

## Kotlin (coroutines)

```kotlin
croniqRunner {
    options(CroniqRunnerOptions.builder().serverUrl("http://localhost:4000").build())

    addJob("billing:invoice") { ctx ->
        // suspend body — call other suspend functions freely
        delay(100)
        callDatabase(ctx.metadata())
    }
}
```

## OpenTelemetry

```java
import io.opentelemetry.api.GlobalOpenTelemetry;
import io.croniq.runner.otel.OpenTelemetryObserver;

var runner = CroniqRunner.builder()
        .options(options)
        .observer(new OpenTelemetryObserver(GlobalOpenTelemetry.get()))
        .addJob("billing:invoice", ...)
        .build();
```

One span per execution with the standard attributes (`croniq.job.key`, `croniq.execution.id`, `croniq.execution.attempt`, `croniq.runner.id`, `croniq.execution.outcome`).

## Toolchain

- **JDK 21+** required. Virtual threads change the concurrency design enough that supporting earlier JDKs would mean two parallel implementations — see issue #133's "Out of scope".
- **Gradle 8.10+** with the Kotlin DSL. The wrapper script (`./gradlew`) pins the version; no global Gradle install is needed.

## Build & test

```sh
cd sdks/java
./gradlew checkAll        # spotless + checkstyle + test on every module
./gradlew formatAll       # apply spotless formatting in place
./gradlew :core:test      # core unit tests only
./gradlew publishToMavenLocal     # smoke-test publishing to ~/.m2
./gradlew :conformance-tests:test # run the wire-protocol suite
```

## Layout

```
sdks/java/
├── settings.gradle.kts            multi-module declaration
├── build.gradle.kts               root tasks (checkAll, formatAll) + nexus-publish
├── gradle/libs.versions.toml      central dependency catalogue
├── buildSrc/                      convention plugins (java/kotlin/publish)
├── config/checkstyle/             style rules
├── core/                          → io.croniq:runner
├── spring-boot-starter/           → io.croniq:runner-spring-boot-starter
├── kotlin-ext/                    → io.croniq:runner-kotlin-ext
├── otel/                          → io.croniq:runner-opentelemetry
└── conformance-tests/             YAML-driven wire-protocol suite
```

This mirrors the reference .NET SDK at [`sdks/dotnet/src/Croniq.Runner.Sdk/`](../dotnet/src/Croniq.Runner.Sdk/) so contributors who know one ecosystem can navigate the other.

## Wire-protocol conformance

The SDK is validated against the shared, language-neutral conformance suite at [`sdks/conformance/`](../conformance/) — the same 12 YAML cases that drive the .NET SDK. Future Python / Go / TypeScript SDKs will pass the same cases.

When the wire protocol gains a new behaviour, the case is added to `sdks/conformance/cases/` first.

## Publishing

Snapshots auto-promote to Sonatype's s01 snapshot repo. Releases are staged via the Sonatype OSSRH endpoint and require manual close + release:

```sh
# release a tagged version (CI sets OSSRH_USERNAME, OSSRH_PASSWORD,
# GPG_SIGNING_KEY, GPG_SIGNING_PASSWORD from Actions secrets)
./gradlew publishToSonatype closeAndReleaseSonatypeStagingRepository
```

Local development never signs — the convention plugin only activates GPG when `GPG_SIGNING_KEY` is set in the environment.

## Compatibility matrix

| SDK Version | Croniq Server (min) | Croniq Server (max tested) | JDK     |
| ----------- | ------------------- | -------------------------- | ------- |
| 0.1.x       | 0.14.0              | 0.14.0                     | 21, 23  |

## License

Dual-licensed under MIT OR Apache-2.0. See [LICENSE-MIT](../../LICENSE-MIT) and [LICENSE-APACHE](../../LICENSE-APACHE) at the repo root.
