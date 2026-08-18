# Changelog

All notable changes to the Python runner SDK are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the package adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **A ceiling on consecutive poll conflicts
  ([#466](https://github.com/nuetzliches/croniq/issues/466)).** New
  `RunnerOptions.max_consecutive_poll_conflicts` (default `3`, range
  `[1, 100]`) budgets consecutive `409 Conflict` responses to
  `POST /v1/work/poll`. On exhaustion `Runner.run()` raises the new
  `PollInstanceConflictError` (exported from the package root), which carries
  `runner_id` and `consecutive_count` and names the remedy: stop the duplicate
  process or rotate the `runner_id`. The counter resets on a successful poll
  or on any non-409 failure (5xx, network, timeout), which say nothing about
  instance ownership.

  **Behaviour change.** A sustained `409` previously retried forever. One
  conflict is still transient — a deposed instance may win its identity back,
  and conformance case 11 pins that it is retried — but a *streak* of them is
  a duplicate deployment, two processes started with the same fixed
  `runner_id`, and retrying that forever left the misconfiguration behind a
  warning that scrolled past. The runner now exits so the process can fail
  non-zero and reach monitoring, matching what the Rust and .NET SDKs have done
  since [#134](https://github.com/nuetzliches/croniq/issues/134) sub-item 1. Set the
  option to `100` to get close to the old behaviour. The `403` half was
  already symmetric across the SDKs (#437/#458); this closes the `409` half.

  Conformance case `16-poll-409-conflict-ceiling.yaml` pins the contract on
  the wire and now runs green in all five bindings — including .NET, whose
  implementation had no corpus coverage until now.

### Fixed

- **A `403` on the work endpoints is fatal
  ([#437](https://github.com/nuetzliches/croniq/issues/437)).** Since server
  issue #436 bound a runner's identity to the authenticated caller,
  `/v1/work/*` answers `403` when the credential does not own the `runner_id`
  the request names. The poll loop retried that forever on
  `poll_retry_delay_ms`, so a fenced-out runner looked idle rather than
  misconfigured. A `403` is permanent — no retry can clear it — so
  `Runner.run()` now raises the new `RunnerOwnershipDeniedError` (exported
  from the package root) on the first one. It carries `runner_id` and names
  both fixes: give the runner its own `runner_id`, or release the existing
  binding with `DELETE /v1/runners/{id}`. The drain and client-close steps in
  the `finally` block still run, so in-flight handlers get their grace period
  before the error reaches the caller.

  A `403` on ack, lease renew or a streaming-log batch is now logged at
  `error` with the same remedy instead of the generic `error`/`debug`/
  `warning`: an unacked execution stays claimed until its lease expires, a
  refused renew means the lease expires mid-handler, and a refused batch means
  the execution produces no log output. Renew's `404`/`409` — routine when a
  renew races the runner's own completion, see server issue #438 — stay at
  `debug`.

### Security

- **HTTPS is required for a non-loopback `server_url`
  ([#440](https://github.com/nuetzliches/croniq/issues/440)).** `server_url`
  defaulted to `http://localhost:4000` and nothing checked the scheme, so
  swapping in a real host shipped the API key as a cleartext `Authorization`
  header on every poll — and, because httpx honours `HTTP_PROXY` by default,
  through any configured proxy as well. `RunnerOptions` and
  `TriggerClientOptions` now validate the URL in `__post_init__`, so a
  misconfiguration fails fast instead of on the first poll. `https://` is
  always accepted; `http://` only when the host is loopback (`localhost`,
  `127.0.0.0/8`, `::1`), keeping the documented `http://localhost:4000`
  quickstart working; anything else raises `ValueError` naming the URL and the
  opt-in. The new `allow_insecure_http=True` option accepts a cleartext URL
  deliberately and logs one loud warning on the `croniq_runner.security`
  logger instead.

- **`job_key` and `execution_id` no longer reach log messages, and are
  validated on ingest
  ([#441](https://github.com/nuetzliches/croniq/issues/441)).** `_runner`
  interpolated both identifiers into its messages
  (`"handler for %s (execution %s) raised"`), so a server-supplied value
  carrying CRLF forged log records and one carrying ANSI escapes reached the
  operator's terminal raw. Both now travel as record attributes via
  `extra={…}` with a constant message — the host `logging` configuration owns
  rendering (a JSON formatter picks them up; a plain `%(message)s` formatter
  ignores them), and the SDK does not escape a second time. The runner
  additionally validates both identifiers before dispatching. A `job_key` is
  refused only for containing a control character — C0, DEL or C1 — or
  exceeding 256 scalar values; every printable character in any script is
  accepted, interior spaces included, because
  `job "billing:monthly invoice" { … }` is legal DSL and `POST /v1/jobs`
  constrains the key not at all. Execution ids keep a narrow
  `a-z A-Z 0-9 - _ . :` charset up to 64 characters, which the server's v4 UUIDs
  satisfy strictly. A refused assignment with a *valid* `execution_id` is acked
  as a failure naming the offending field, so it dead-letters rather than
  looping; one whose `execution_id` is itself unsafe is dropped, since nothing
  safely addresses the server.
- **The per-job logger namespace is gone
  ([#441](https://github.com/nuetzliches/croniq/issues/441)).**
  `ExecutionContext` built `logging.getLogger(f"croniq_runner.job.{job_key}")`,
  handing a server control of a logger namespace. `getLogger` caches every name
  forever, plus a `PlaceHolder` per dot-separated ancestor, so a server
  delivering many distinct keys grew the process without bound — and a key
  chosen to land under a namespace configured with `propagate=False` evaded log
  filtering. `ctx.logger` is now a `LoggerAdapter` over the fixed
  `croniq_runner.job` logger, attaching `job_key`, `execution_id`, `runner_id`
  and `attempt` to every record. Logging configuration written against
  `croniq_runner.job.<key>` needs to move to `croniq_runner.job`.

- **An injected HTTP client no longer discards the configured credential
  ([#443](https://github.com/nuetzliches/croniq/issues/443)).** `CroniqClient`
  baked the `Authorization` header into the `httpx.AsyncClient` it built for
  itself, so passing `http=` — the documented path for mTLS, proxies and custom
  transports — produced a client carrying no credential at all, and every
  runner request went out unauthenticated. Against a correct server that fails
  closed (401, then the retry loop), but if the injected client carried its own
  broader `Authorization`, `RunnerOptions.api_key` was silently ignored and the
  runner authenticated with the wrong credential. Auth is now applied per
  request at all five call sites — `poll`, `ack`, `renew`, `push_events`,
  `register_job` — matching what `TriggerClient` already did, which also means
  the configured credential overrides any `Authorization` the injected client
  sets rather than losing to it.

- **The quickstart reads the API key from the environment
  ([#443](https://github.com/nuetzliches/croniq/issues/443)).** `README.md` and
  the `croniq_runner` / `TriggerClient` docstrings showed
  `api_key="croniq_..."` inline while the Go and TypeScript samples used
  `os.Getenv` / `process.env`. They now read `os.environ["CRONIQ_API_KEY"]`
  and `os.environ["CRONIQ_TRIGGER_KEY"]`, so copy-pasting the sample does not
  land a literal key in source control. Documentation only.

## [0.3.0] - 2026-07-18

### Added

- **`ExecutionContext.scheduled_for`** exposes the trigger's original logical
  fire time (`datetime | None`), stable across retries and dead-letter replays.
  Use it for time-relative job logic (e.g. the month a report covers) instead of
  `datetime.now()`. `None` when the server predates the field — the SDK never
  falls back to the queue fire time.

## [0.2.0] - 2026-07-15

### Added

- **First-class producer trigger client**
  ([#283](https://github.com/nuetzliches/croniq/issues/283)). `TriggerClient` —
  configured via `TriggerClientOptions`, independent of the runner and carrying
  its own `jobs:trigger`-scoped credentials — wraps `POST /v1/trigger`.
  `await client.trigger(job_key, ...)` returns a `TriggerResult`
  (`execution_id`, `queued`, `deduplicated`); an optional `idempotency_key`
  drives server-side dedup
  ([#279](https://github.com/nuetzliches/croniq/issues/279)), with a missing
  `deduplicated` flag parsed as `False`. Non-2xx responses raise — including the
  oversized-idempotency-key `400` and the per-job queue-overflow `429`
  ([#299](https://github.com/nuetzliches/croniq/issues/299)).

## [0.1.0] - 2026-05-25

### Added

- Initial release of `croniq-runner` for Python 3.11+.
- Async-first runner (`Runner.run`) over `httpx.AsyncClient`.
- Pydantic v2 DTOs mirroring `openapi.yaml` snake_case wire format.
- Streaming `LogWriter` backed by a bounded `asyncio.Queue` with batching
  (32 events / 200 ms / max 100 per POST) and drain-before-ack guarantee.
- Server-side cancellation via `PollResponse.cancel` honoured per-execution.
- Lease-renewal heartbeat at `renew_interval` while a handler is in flight.
- Self-registration via `POST /v1/jobs/register` for handlers declared with
  a `schedule=` argument.
- Authentication: `Authorization: ApiKey <key>` (preferred) or
  `Authorization: Bearer <token>`.
- Conformance binding under `tests/conformance/` driving the language-agnostic
  YAML suite at [`sdks/conformance/cases/`](../conformance/cases) — one pytest
  per case, runs against `pytest-httpserver`.
- Optional OpenTelemetry tracing via the `croniq-runner[otel]` extra; spans
  emitted around each execution when `opentelemetry-api` is importable.
- **First-class trigger (producer) client
  ([#283](https://github.com/nuetzliches/croniq/issues/283)),** at parity with
  the .NET producer client
  ([#277](https://github.com/nuetzliches/croniq/issues/277)). `TriggerClient`
  (configured with `TriggerClientOptions`) wraps `POST /v1/trigger`:
  `await client.trigger(job_key, metadata=…, require=…, prefer=…, timeout=…,
  idempotency_key=…)` returns `TriggerResult(execution_id, queued,
  deduplicated)`. It is independent of `Runner` and carries its **own**
  credentials, because triggering needs the `jobs:trigger` (or `admin`) scope,
  distinct from runner poll keys. Unset optionals are omitted from the request
  body (never sent as `null`); `metadata` is forwarded as arbitrary nested JSON.
  The optional `idempotency_key` enables server-side trigger dedup
  ([#279](https://github.com/nuetzliches/croniq/issues/279)) — `deduplicated` is
  surfaced from the response and defaults to `False` on servers that omit it.
  Non-2xx responses raise `httpx.HTTPStatusError`, including the per-job
  queue-overflow `429` from
  [#299](https://github.com/nuetzliches/croniq/issues/299). Validated against the
  shared trigger conformance suite
  ([#287](https://github.com/nuetzliches/croniq/issues/287)), now wired into the
  Python binding under `tests/conformance/`.
