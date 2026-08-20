# Changelog — Croniq Runner SDK (.NET)

All notable changes to the .NET Runner SDK packages are documented in this file. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The .NET SDK uses its own version track separate from the Croniq server. SDK versions are tagged as `dotnet-sdk-v*` (e.g. `dotnet-sdk-v0.1.0`).

## [Unreleased]

### Added

- **A ceiling on consecutive authentication failures
  ([#473](https://github.com/nuetzliches/croniq/issues/473)).** New `CroniqRunnerOptions.MaxConsecutiveAuthFailures` (default `3`, range `[1, 100]`) budgets
  consecutive `401 Unauthorized` responses to `POST /v1/work/poll`. On
  exhaustion the runner stops with the new `AuthFailedException`, which carries the streak length and
  names the remedy: restart with the current key. The counter resets on a
  successful poll and on any other failure — a 5xx says nothing about whether
  the credential is valid.

  **Behaviour change.** A `401` was previously classified as transient, so a
  runner whose key was revoked retried it every poll interval forever. The
  credential is read once, at construction, and never re-read, so retrying
  could not clear it: the process stayed up, looked healthy, did nothing, and
  never exited non-zero — which meant no supervisor ever restarted it, and
  restarting is exactly what would have picked up the new key.

  Unlike the `403` of #437 the first `401` is *not* fatal. Key rotation hands
  over by installing the new key and giving the old one an expiry
  ([#471](https://github.com/nuetzliches/croniq/issues/471)), so dying on a
  single rejection would turn a narrow race around that handover into an
  outage. Conformance case `17-poll-401-auth-ceiling.yaml` pins the contract on
  the wire across all five runner bindings.

### Fixed

- **A `403` on the work endpoints is fatal
  ([#437](https://github.com/nuetzliches/croniq/issues/437)).** Since server
  issue #436 bound a runner's identity to the authenticated caller,
  `/v1/work/*` answers `403` when the credential does not own the `runner_id`
  the request names. The poll loop retried that forever on `PollRetryDelay`,
  so a fenced-out runner looked idle rather than misconfigured. A `403` is
  permanent — no retry can clear it — so `UpdateConflictStreak` now bails on
  the first one (an effective threshold of 1, independent of
  `MaxConsecutivePollConflicts`) and `RunAsync` throws the new
  `RunnerOwnershipDeniedException`, which carries `RunnerId` and names both
  fixes: give the runner its own `runner_id`, or release the existing binding
  with `DELETE /v1/runners/{id}`. The `409` conflict-streak path is unchanged,
  and the streak counter is deliberately left untouched by a `403` — it
  reports how long a duplicate deployment has been fenced out, which a `403`
  says nothing about.

  A `403` on ack, lease renew or a streaming-log batch is now logged at
  `LogError` with the same remedy instead of being flattened into the generic
  failure (`LogDebug` for renew, `LogWarning` for log batches). Each has a
  distinct consequence worth naming: an unacked execution stays claimed until
  its lease expires, a refused renew means the lease expires mid-handler, and
  a refused batch means the execution produces no log output. Renew's
  `404`/`409` — routine when a renew races the runner's own completion, see
  server issue #438 — are now matched explicitly and stay at `LogDebug`.

### Security

- **HTTPS is required for a non-loopback `ServerUrl`
  ([#440](https://github.com/nuetzliches/croniq/issues/440)).** `ServerUrl`
  defaulted to `http://localhost:4000` and `[Required, Url]` accepted any
  `http://` host, so swapping in a real host shipped the API key as a
  cleartext `Authorization` header on every poll, with no warning. Both
  `CroniqRunnerOptions` and `CroniqClientOptions` now validate the scheme
  during options validation — i.e. at host startup via the existing
  `ValidateOnStart()`, not on the first request. `https://` is always
  accepted; `http://` only when the host is loopback (`localhost`,
  `127.0.0.0/8`, `::1`), so the documented `http://localhost:4000` quickstart
  keeps working; anything else fails with an `OptionsValidationException`
  naming the URL and the opt-in. The new `AllowInsecureHttp` option property
  (bindable from `Croniq:Runner` / `Croniq:Client`) accepts a cleartext URL
  deliberately and logs one loud warning under the
  `Croniq.Runner.Sdk.Security` category instead.

- **`job_key` and `execution_id` no longer reach log messages, and are
  validated on ingest
  ([#441](https://github.com/nuetzliches/croniq/issues/441)).**
  `ExecutionDispatcher` interpolated both identifiers into its message
  templates ("handler for {JobKey} (execution {ExecutionId}) threw"), so a
  server-supplied value carrying CRLF forged log records and one carrying ANSI
  escapes reached the operator's terminal raw. Both now travel as `ILogger`
  scope state with a constant message — the configured provider owns rendering,
  and the SDK does not escape a second time. Set `IncludeScopes = true` on the
  console formatter (or use any structured sink) to see them.
  The runner additionally validates both identifiers before dispatching. A
  `job_key` is refused only for containing a control character — C0, DEL or C1
  — or exceeding 256 scalar values; every printable character in any script is
  accepted, interior spaces included, because
  `job "billing:monthly invoice" { … }` is legal DSL and `POST /v1/jobs`
  constrains the key not at all. Execution ids keep a narrow
  `a-z A-Z 0-9 - _ . :` charset up to 64 characters, which the server's v4 UUIDs
  satisfy strictly. A refused assignment with a *valid* `execution_id` is acked
  as a failure naming the offending field, so it dead-letters rather than
  looping; one whose `execution_id` is itself unsafe is dropped, since nothing
  safely addresses the server.
- **The per-job logger category is gone
  ([#441](https://github.com/nuetzliches/croniq/issues/441)).** The dispatcher
  called `CreateLogger($"CroniqJob.{jobKey}")`, handing a server control of a
  logger category. `ILoggerFactory` caches categories permanently, so a server
  delivering many distinct keys grew the process without bound, and some sinks
  map a category to a filename. The handler logger is now the fixed category
  `CroniqJob` with `job_key` carried as scope state. Filtering rules written
  against `CroniqJob.<key>` need to move to the `CroniqJob` category.

- **The health-check description no longer echoes exception text
  ([#443](https://github.com/nuetzliches/croniq/issues/443)).** A failed poll
  put `ex.Message` into the runner state probe and `CroniqRunnerHealthCheck`
  rendered it into the result description. `HttpRequestException` and
  `SocketException` messages routinely embed the resolved host and port
  ("No such host is known. (croniq.internal:4000)"), so an unauthenticated
  reader of `/health` learned the internal Croniq hostname. No credential was
  ever exposed — API keys never appear in these messages — and the stock
  ASP.NET Core response writer emits only the aggregate status, so this
  surfaced only behind a custom or dashboard response writer. The description
  now carries a fixed category derived from the exception *type* —
  `connection failed`, `http status <code>`, `poll timed out` or `poll failed`,
  produced by the new `internal CroniqRunner.DescribePollFailure` — while the
  full `ex.Message` stays in the log line, which is operator-only. The wording
  changed from `(error: …)` to `(reason: …)`; anything parsing the description
  string should be checked. `IRunnerStateProbe.LastPollError` was renamed
  `LastPollFailureReason` (both `internal`, so no public API changed) to make
  the invariant readable at the type.

- **The OpenTelemetry audit suppressions no longer apply to the main SDK
  ([#443](https://github.com/nuetzliches/croniq/issues/443)).**
  `Directory.Build.props` applied four `NuGetAuditSuppress` entries to every
  project in the tree, but `Croniq.Runner.Sdk` references no OpenTelemetry
  package — a suppression on a project that cannot hit the advisory only
  degrades that project's audit signal, and would silently swallow the same
  GHSA arriving later through an unrelated dependency. The list stays in one
  place but is now conditioned on a `CroniqUsesOpenTelemetry` property that
  only `Croniq.Runner.Sdk.OpenTelemetry` and the demo app set. Build-time
  configuration only; no shipped assembly changed.

### Added

- **Scoped shell-handler registration
  ([#442](https://github.com/nuetzliches/croniq/issues/442)).**
  `AddCroniqShellHandler("deploy:run", …)` registers the shell-exec handler
  for the listed job keys only, instead of as the catch-all for every job key
  the server dispatches. The parameterless overload keeps its catch-all
  semantics as a documented opt-in; the scoped form is now the recommended
  registration. New `CroniqShellHandlerOptions` (configurable via
  `AddCroniqShellHandler(o => …, keys)` overloads) carries
  `AllowUnsafeEnvironment` (default `false`).

### Fixed

- **POSIX shell commands are no longer corrupted by quoting
  ([#442](https://github.com/nuetzliches/croniq/issues/442)).** The handler
  interpolated the command into `/bin/sh -c "…"`, escaping `"` but not `\`, so
  commands containing escaped quotes or ending in a backslash reached `sh`
  altered. The command now travels as a single argv entry via
  `ProcessStartInfo.ArgumentList`, mirroring the Rust shell runner. The
  Windows branch deliberately keeps the raw `cmd.exe /c <command>`
  pass-through (with a pinning test), because `cmd` parses the remainder of
  the line itself and argv-quoting would corrupt it.
- **The `user` directive fails closed instead of being silently ignored
  ([#442](https://github.com/nuetzliches/croniq/issues/442)).** .NET cannot
  switch the subprocess user, so a payload that sets `user` now fails the
  execution with `user directive is not supported by the .NET shell handler`
  rather than running the command as the runner's own user.
- **Payload-supplied `env` names that hijack process resolution or library
  loading are rejected
  ([#442](https://github.com/nuetzliches/croniq/issues/442)).** `PATH`,
  `PATHEXT`, `COMSPEC`, `LD_PRELOAD`, `LD_LIBRARY_PATH`, `DYLD_*` and
  `CRONIQ_*` (case-insensitive) fail the execution unless
  `CroniqShellHandlerOptions.AllowUnsafeEnvironment` is set.

## [0.5.0] - 2026-07-18

### Added

- **`CroniqExecutionContext.ScheduledFor`** exposes the trigger's original logical fire time (`DateTimeOffset?`), stable across retries and dead-letter replays. Use it for time-relative job logic (e.g. the month a report covers) instead of `DateTimeOffset.UtcNow`. `null` when the server predates the field — the SDK never falls back to the queue fire time.

## [0.4.0] - 2026-07-15

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
