# Changelog

## Unreleased

### Security

- **HTTPS is required for a non-loopback `serverUrl` ([#440](https://github.com/nuetzliches/croniq/issues/440)).** `serverUrl` was only checked for parseability, so an operator who kept the documented `http://localhost:4000` shape and swapped in a real host shipped the API key as a cleartext `Authorization` header on every poll — and, because undici honours `HTTP_PROXY` by default, through any configured proxy as well. `resolveOptions` (i.e. `createRunner`) and the `CroniqTriggerClient` constructor now validate the scheme up front, so a misconfiguration fails fast instead of on the first poll. `https://` is always accepted; `http://` only when the host is loopback (`localhost`, `127.0.0.0/8`, `::1`, including `[::1]`), keeping the documented quickstart working; anything else throws a `TypeError` naming the URL and the opt-in. The new `allowInsecureHttp: true` option accepts a cleartext URL deliberately and emits one loud `logger.warn` instead. `CroniqTriggerClientOptions` gained a `logger` option so that warning can be routed; `isLoopbackHostname` is exported for callers that want the same classification.

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
