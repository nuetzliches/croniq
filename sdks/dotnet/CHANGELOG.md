# Changelog — Croniq Runner SDK (.NET)

All notable changes to the .NET Runner SDK packages are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The .NET SDK uses its own version track separate from the Croniq server. SDK versions are tagged as `dotnet-sdk-v*` (e.g. `dotnet-sdk-v0.1.0`).

## [Unreleased]

### Added
- **`LogWriter` accepts `TimeProvider` for deterministic flush testing
  ([#134](https://github.com/nuetzliches/croniq/issues/134) sub-item 3).**
  The internal flusher's `PeriodicTimer` is now driven through the same
  `TimeProvider` pipeline `CroniqRunner` and the health check already
  use. Production code paths default to `TimeProvider.System` and stay
  byte-equivalent. Unit tests wire `Microsoft.Extensions.Time.Testing.FakeTimeProvider`
  to advance the 200 ms batch-time threshold deterministically — the
  partial-batch flush path that conformance case 10
  (`10-streaming-logs-time-threshold.yaml`) can only assert with
  `min_count: 1` because of `Task.WhenAny`'s read-bias under real-time
  scheduling is now reliably covered in
  `Croniq.Runner.Sdk.Tests.LogWriterTests`.

- Initial `Croniq.Runner.Sdk` package with poll/ack/renew/events loop, Generic Host integration, options-pattern configuration, server-side cancellation wiring, and streaming `ILogWriter` backed by `System.Threading.Channels`.
- Initial `Croniq.Runner.Sdk.OpenTelemetry` package with `ActivitySource`/`Meter` constants and `Add…Instrumentation()` extensions.
- Multi-target build for `net8.0` (LTS) and `net10.0` (LTS).
- Health check (`AddCroniqRunnerHealthCheck`), shell-exec decoder (`AddCroniqShellHandler`), and demo runner under `examples/CroniqRunner.Demo`.
- Language-agnostic conformance suite at [`sdks/conformance/`](../conformance/) — 12 YAML cases covering poll/ack/renew, server-initiated cancel, drain, lease renewal, streaming logs, auth header, self-register, 409 conflict, and 5xx backoff. The .NET binding lives at `tests/Croniq.Runner.Sdk.Conformance.Tests/` and exercises every case via WireMock.Net.

### Changed
- **Drain semantics**: host shutdown no longer cancels in-flight handlers immediately. The per-execution `CancellationTokenSource` is unlinked from the outer poll token, so handlers run to natural completion within `DrainTimeout` and only get hard-cancelled if that budget is exhausted. Matches the Rust SDK and what most graceful-shutdown stories expect.

### Infrastructure
- `.NET SDK CI` GitHub Actions workflow with four jobs (`schema`, `build` matrix, `conformance`, `pack-smoke`) plus a `required` aggregator so branch-protection only needs to gate one job name.
- `.gitattributes` pins LF line endings for `sdks/dotnet/**` and `sdks/conformance/**` so `dotnet format --verify-no-changes` succeeds on Windows runners.
