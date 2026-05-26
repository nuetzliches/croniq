# Changelog

## 0.1.0 - Unreleased

Initial release. Implements the Croniq runner protocol:

- Poll / ack / renew / events / register endpoints (`/v1/work/*`, `/v1/jobs/register`).
- ESM-first, Node 20+, native `fetch` and `AbortController`.
- Streaming `LogWriter` with batch-by-count, batch-by-time, drain-before-ack semantics.
- Per-execution `AbortSignal` honouring `PollResponse.cancel`.
- Self-registration of schedule-bearing handlers at startup.
- `Authorization: ApiKey {key}` and `Bearer {token}` precedence.
- Persistent runner-id resolution (env → state file → generated).
- Conformance against all 12 cases in [`sdks/conformance/cases/`](../conformance/cases).
