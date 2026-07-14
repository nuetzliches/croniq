# Changelog — Croniq Runner SDK (.NET)

All notable changes to the .NET Runner SDK packages are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The .NET SDK uses its own version track separate from the Croniq server. SDK versions are tagged as `dotnet-sdk-v*` (e.g. `dotnet-sdk-v0.1.0`).

## [Unreleased]

### Added

- **Genuine trim/AOT compatibility — the package now declares
  `IsAotCompatible`/`IsTrimmable`
  ([#295](https://github.com/nuetzliches/croniq/issues/295)).** These flags
  were dead configuration since day one (they lived in a
  `$(IsPackable)`-conditioned group in `Directory.Build.props`, evaluated
  before the .csproj set `IsPackable`, so the trim/AOT analyzers never ran).
  Enabling them surfaced 22 findings in the DI/options layer, now all fixed:
  - `IConfiguration` binding in `AddCroniqRunner`/`AddCroniqClient` goes
    through the source-generated configuration binder
    (`EnableConfigurationBindingGenerator`) instead of reflection
    (was IL2026/IL3050).
  - `ValidateDataAnnotations()` is replaced by source-generated
    `[OptionsValidator]` validators (`CroniqRunnerOptionsValidator`,
    `CroniqClientOptionsValidator`), keeping options validation
    reflection-free (was IL2026).
  - The `THandler` type parameter on the `AddCroniqJob<THandler>(...)` /
    `AddCroniqDefaultHandler<THandler>()` overloads now carries
    `[DynamicallyAccessedMembers(PublicConstructors)]` so the trimmer
    preserves handler constructors for DI activation (was IL2091).

  The flags now live in `Directory.Build.targets` (where `IsPackable` is
  final), so the analyzers run on every build and `TreatWarningsAsErrors`
  turns any future regression into a hard error. The JSON wire layer was
  already source-generated. No behavioural or API changes; existing
  `ValidateOnStart()` validation is unchanged.

## [0.3.1] - 2026-07-06

### Fixed

- **Package metadata and symbol packages actually ship now.** The
  `$(IsPackable)`-conditioned PropertyGroups (license expression, readme,
  icon, tags, project URL, SourceLink, `IncludeSymbols`/`snupkg`, MinVer tag
  prefix) lived in `Directory.Build.props`, which is evaluated *before* the
  .csproj sets `IsPackable` — the conditions silently never matched, so
  0.1.0–0.3.0 were published without that metadata and without `.snupkg`
  symbol packages (this is also what failed the `dotnet-sdk-v0.2.0` and
  `dotnet-sdk-v0.3.0` release workflows at the symbol-push step). The groups
  now live in `Directory.Build.targets`, where `IsPackable` is final.
  Functionally identical binaries to 0.3.0.
- The never-applied `IsAotCompatible`/`IsTrimmable` flags are intentionally
  left off: enabling them surfaces 22 genuine trim/AOT findings in the
  DI/options layer. Proper annotation work is tracked in
  [#295](https://github.com/nuetzliches/croniq/issues/295).

## [0.3.0] - 2026-07-06

### Added

- **First-class trigger (producer) client
  ([#277](https://github.com/nuetzliches/croniq/issues/277)).**
  `services.AddCroniqClient(...)` registers `ICroniqTriggerClient`, whose
  `TriggerAsync(jobKey, metadata?, require?, prefer?, timeout?, idempotencyKey?, ct)`
  wraps `POST /v1/trigger` and returns `TriggerResult { ExecutionId, Queued,
  Deduplicated }`. The client is independent of `AddCroniqRunner` and carries
  its own credentials (`Croniq:Client` section) because triggering requires
  the `jobs:trigger` (or `admin`) scope, distinct from runner poll keys.
  Registration is idempotent, mirroring `AddCroniqRunner`. The optional
  `idempotencyKey` is forwarded as `idempotency_key` for server-side trigger
  dedup ([#279](https://github.com/nuetzliches/croniq/issues/279)); servers
  without support ignore it and `Deduplicated` stays `false`.

### Infrastructure

- Transitive pin of `Scriban.Signed` to 7.2.5 in the test projects —
  WireMock.Net 1.6.7 brings 5.5.0, which trips NuGet audit via
  [GHSA-24c8-4792-22hx](https://github.com/advisories/GHSA-24c8-4792-22hx)
  (build-breaking with `TreatWarningsAsErrors`). Test-only dependency; the
  shipped packages are unaffected.

## [0.2.0] - 2026-05-28

### Fixed

- **`AddCroniqRunner(...)` is now idempotent
  ([#221](https://github.com/nuetzliches/croniq/issues/221)).** Calling it
  more than once on the same `IServiceCollection` — natural when several
  feature modules each contribute jobs — previously duplicated the options
  `Bind`, the `CroniqAuthHandler` in the HTTP pipeline, and the hosted
  service: `Capabilities` ended up as `["worker", "worker"]`, every poll
  request carried a comma-joined `Authorization` header, and the server
  returned 401 with no useful diagnostics. The second and subsequent calls
  now no-op for the shared setup and return a builder that still accepts
  further `.AddCroniqJob<T>(...)` chaining. A defensive fix in
  `CroniqAuthHandler` also strips any pre-existing `Authorization` header
  before writing its own, so an upstream code path that already set the
  header can no longer produce a comma-joined value.

### Changed

- **Poll 409 Conflict is fatal after `MaxConsecutivePollConflicts`
  (default 3) consecutive responses
  ([#134](https://github.com/nuetzliches/croniq/issues/134) sub-item 1).**
  Today every poll failure (including the 409 the server returns when
  another runner is already registered with the same `runner_id`) is
  retried with `PollRetryDelay` backoff, forever. The new behaviour
  counts *consecutive* 409s; after N the runner throws
  `PollInstanceConflictException` (new public type) out of `RunAsync`,
  so the host process exits with a non-zero status code instead of
  looping silently. A successful poll or a non-409 transient error
  (5xx, timeout) resets the counter, so a recovered 5xx doesn't
  accumulate against the conflict budget. New option
  `CroniqRunnerOptions.MaxConsecutivePollConflicts` (int, `[Range(1, 100)]`)
  is exposed via the standard options-pattern binding (env / JSON
  config / inline `AddCroniqRunner(o => o.MaxConsecutivePollConflicts = …)`).

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

## [0.1.0] - 2026-05-26

### Added

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
