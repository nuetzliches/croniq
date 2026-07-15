# Changelog

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
