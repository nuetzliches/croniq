# Changelog

## Unreleased

### Added

- **A ceiling on consecutive poll conflicts ([#466](https://github.com/nuetzliches/croniq/issues/466)).** New `maxConsecutivePollConflicts` option (default `3`, range `[1, 100]`) budgets consecutive `409 Conflict` responses to `POST /v1/work/poll`. On exhaustion `run()` rejects with the new exported `PollInstanceConflictError`, which carries `runnerId` and `consecutiveCount` and names the remedy: stop the duplicate process or rotate the `runner_id`. The counter resets on a successful poll or on any non-409 failure (5xx, network, timeout), which say nothing about instance ownership.

  **Behaviour change.** A sustained `409` previously retried forever. One conflict is still transient — a deposed instance may win its identity back, and conformance case 11 pins that it is retried — but a *streak* of them is a duplicate deployment, two processes started with the same fixed `runnerId`, and retrying that forever left the misconfiguration behind a warning that scrolled past. The runner now exits so the process can fail non-zero and reach monitoring, matching what the Rust and .NET SDKs have done since [#134](https://github.com/nuetzliches/croniq/issues/134) sub-item 1. Set the option to `100` to get close to the old behaviour. The `403` half was already symmetric across the SDKs (#437/#458); this closes the `409` half.

  Conformance case `16-poll-409-conflict-ceiling.yaml` pins the contract on the wire and now runs green in all five bindings — including .NET, whose implementation had no corpus coverage until now.

### Fixed

- **A `403` on the work endpoints is fatal ([#437](https://github.com/nuetzliches/croniq/issues/437)).** Since server issue #436 bound a runner's identity to the authenticated caller, `/v1/work/*` answers `403` when the credential does not own the `runner_id` the request names. The poll loop retried that forever on `pollRetryDelayMs`, so a fenced-out runner looked idle rather than misconfigured. A `403` is permanent — no retry can clear it — so `run()` now rejects with the new exported `RunnerOwnershipDeniedError` on the first one. It carries `runnerId` and names both fixes: give the runner its own `runner_id`, or release the existing binding with `DELETE /v1/runners/{id}`. The drain step still runs first, so in-flight handlers get their grace period before the rejection surfaces.

- **A `403` on ack, renew or log events is visible ([#437](https://github.com/nuetzliches/croniq/issues/437)).** These paths never inspected the status, so an ownership refusal was indistinguishable from a 5xx: `logger.debug` for renew, `logger.warn` for a dropped log batch. Each now logs at `logger.error` with the remedy, because each has its own consequence — an unacked execution stays claimed until its lease expires, a refused renew means the lease expires mid-handler, and a refused batch means the execution produces no log output. Renew's `404`/`409` — routine when a renew races the runner's own completion, see server issue #438 — stay at `debug`.

### Security

- **HTTPS is required for a non-loopback `serverUrl` ([#440](https://github.com/nuetzliches/croniq/issues/440)).** `serverUrl` was only checked for parseability, so an operator who kept the documented `http://localhost:4000` shape and swapped in a real host shipped the API key as a cleartext `Authorization` header on every poll — and, because undici honours `HTTP_PROXY` by default, through any configured proxy as well. `resolveOptions` (i.e. `createRunner`) and the `CroniqTriggerClient` constructor now validate the scheme up front, so a misconfiguration fails fast instead of on the first poll. `https://` is always accepted; `http://` only when the host is loopback (`localhost`, `127.0.0.0/8`, `::1`, including `[::1]`), keeping the documented quickstart working; anything else throws a `TypeError` naming the URL and the opt-in. The new `allowInsecureHttp: true` option accepts a cleartext URL deliberately and emits one loud `logger.warn` instead. `CroniqTriggerClientOptions` gained a `logger` option so that warning can be routed; `isLoopbackHostname` is exported for callers that want the same classification.

- **`job_key` and `execution_id` no longer reach log messages, and are validated
  on ingest ([#441](https://github.com/nuetzliches/croniq/issues/441)).**
  `dispatcher.ts`, `runner.ts` and `log-writer.ts` interpolated both identifiers
  into their messages via template literals, and `consoleLogger` then wrote the
  message to the console verbatim — so a server-supplied value carrying CRLF
  forged log records and one carrying ANSI escapes reached the operator's
  terminal raw. Both identifiers now travel only in the `fields` map, which is
  rendered with `JSON.stringify` and therefore already escaped, and
  `consoleLogger` escapes control characters (C0 including ESC, DEL, and C1) in
  the message before writing. The runner additionally validates both identifiers
  before dispatching. A `job_key` is refused only for containing a control
  character — C0, DEL or C1 — or exceeding 256 scalar values; every printable
  character in any script is accepted, interior spaces included, because
  `job "billing:monthly invoice" { … }` is legal DSL and `POST /v1/jobs`
  constrains the key not at all. Execution ids keep a narrow
  `a-z A-Z 0-9 - _ . :` charset up to 64 characters, which the server's v4
  UUIDs satisfy strictly. A refused assignment with a *valid* `execution_id` is
  acked as a failure naming the offending field, so it dead-letters rather than
  looping; one whose `execution_id` is itself unsafe is dropped, since nothing
  safely addresses the server.

## 0.3.0 - 2026-07-18

- **`ExecutionContext.scheduledFor`** exposes the trigger's original logical fire time (`Date | null`), stable across retries and dead-letter replays. Use it for time-relative job logic (e.g. the month a report covers) instead of `new Date()`. `null` when the server predates the field — the SDK never falls back to the queue fire time.

## 0.2.0 - 2026-07-15

- First-class producer **trigger client** ([#284](https://github.com/nuetzliches/croniq/issues/284), parity with the .NET SDK [#277](https://github.com/nuetzliches/croniq/issues/277)): `createTriggerClient(...)` / `CroniqTriggerClient.trigger(jobKey, { metadata, require, prefer, timeout, idempotencyKey })` wraps `POST /v1/trigger` and returns `{ executionId, queued, deduplicated }`. Independent of the runner and carries its own `jobs:trigger`-scoped credentials. Unset optionals are omitted from the body; `idempotency_key` drives server-side dedup ([#279](https://github.com/nuetzliches/croniq/issues/279)) with a missing `deduplicated` flag parsed as `false`. A `429` per-job queue-overflow ([#299](https://github.com/nuetzliches/croniq/issues/299)) surfaces as `QueueOverflowError` (subclass of `HttpError`, with `retryAfterMs`); other non-2xx surface as `HttpError`.
- Conformance against the producer trigger cases in [`sdks/conformance/cases-trigger/`](../conformance/cases-trigger) ([#287](https://github.com/nuetzliches/croniq/issues/287)).

## 0.1.0 - 2026-05-25

Initial release. Implements the Croniq runner protocol:

- Poll / ack / renew / events / register endpoints (`/v1/work/*`, `/v1/jobs/register`).
- ESM-first, Node 20+, native `fetch` and `AbortController`.
- Streaming `LogWriter` with batch-by-count, batch-by-time, drain-before-ack semantics.
- Per-execution `AbortSignal` honouring `PollResponse.cancel`.
- Self-registration of schedule-bearing handlers at startup.
- `Authorization: ApiKey {key}` and `Bearer {token}` precedence.
- Persistent runner-id resolution (env → state file → generated).
- Conformance against every runner case in [`sdks/conformance/cases/`](../conformance/cases).
