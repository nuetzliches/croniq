# Croniq Runner SDK for Java

[![Maven Central](https://img.shields.io/maven-central/v/io.croniq/runner.svg)](https://central.sonatype.com/artifact/io.croniq/runner)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Build job execution runners for [Croniq](https://github.com/nuetzliches/croniq) in Java (21+) or Kotlin. The SDK polls a Croniq server for work, dispatches typed handlers, streams structured logs back, and reports completion — using virtual threads (Project Loom), `java.net.http.HttpClient`, Jackson, and SLF4J only.

> **Status:** PR-1 of [#133](https://github.com/nuetzliches/croniq/issues/133) — Gradle skeleton, CI, lint config. No runtime behaviour yet. The poll/ack loop lands in PR-2.

## Modules

| Coordinates                                       | Purpose                                                                       |
| ------------------------------------------------- | ----------------------------------------------------------------------------- |
| `io.croniq:runner`                                | Core SDK. JDK 21 + Jackson + SLF4J. No Spring / Kotlin dependency.            |
| `io.croniq:runner-spring-boot-starter`            | Opt-in Spring Boot 3 auto-config, `@CroniqJob`, `croniq.runner.*` properties. |
| `io.croniq:runner-kotlin-ext`                     | Coroutine adapters + Kotlin DSL on top of the Java core.                      |
| `conformance-tests` (not published)               | Runs `sdks/conformance/cases/*.yaml` against the SDK.                         |

The `io.croniq:runner-opentelemetry` opt-in instrumentation module is planned for PR-7 and is not yet wired into the build.

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
├── core/                          → io.croniq:runner
├── spring-boot-starter/           → io.croniq:runner-spring-boot-starter
├── kotlin-ext/                    → io.croniq:runner-kotlin-ext
└── conformance-tests/             YAML-driven wire-protocol suite
```

This mirrors the reference .NET SDK at [`sdks/dotnet/src/Croniq.Runner.Sdk/`](../dotnet/src/Croniq.Runner.Sdk/) so contributors who know one ecosystem can navigate the other.

## Wire-protocol conformance

The SDK is validated against the shared, language-neutral conformance suite at [`sdks/conformance/`](../conformance/) — the same 12 YAML cases that drive the .NET SDK. Future Python / Go / TypeScript SDKs will pass the same cases.

When the wire protocol gains a new behaviour, the case is added to `sdks/conformance/cases/` first.

## Compatibility matrix

| SDK Version | Croniq Server (min) | Croniq Server (max tested) | JDK     |
| ----------- | ------------------- | -------------------------- | ------- |
| 0.1.x       | 0.14.0              | 0.14.0                     | 21, 23  |

## License

Dual-licensed under MIT OR Apache-2.0. See [LICENSE-MIT](../../LICENSE-MIT) and [LICENSE-APACHE](../../LICENSE-APACHE) at the repo root.
