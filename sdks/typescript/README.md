# Croniq Runner SDK for Node.js

[![npm](https://img.shields.io/npm/v/%40nuetzliches%2Fcroniq-runner.svg)](https://www.npmjs.com/package/@nuetzliches/croniq-runner)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Build job execution runners for [Croniq](https://github.com/nuetzliches/croniq) in Node.js / TypeScript. The SDK polls a Croniq server for work, dispatches handlers, streams structured logs back, and reports completion — using native `fetch` and `AbortController`, no extra HTTP dependency.

Aimed at workloads that are Node-native: headless-browser automation (Playwright / Puppeteer), web scraping, npm-ecosystem builds, anything that lives most naturally in a JS toolchain.

## Install

```sh
npm install @nuetzliches/croniq-runner
```

(The package name is org-scoped under `@nuetzliches` to align with the project's other npm artefacts. Import path: `import { createRunner } from '@nuetzliches/croniq-runner';`.)

Requires Node.js ≥ 18 (ESM, native `fetch`, `AbortController`).

## Quick start

```ts
import { createRunner } from '@nuetzliches/croniq-runner';

const runner = createRunner({
  serverUrl: 'http://localhost:4000',
  apiKey: process.env.CRONIQ_API_KEY,
  capabilities: ['demo'],
  tags: ['env=dev', 'lang=typescript'],
  maxInflight: 5,
});

runner.handle('hello:world', async (ctx) => {
  ctx.logger.info(`Hello from ${ctx.jobKey} (attempt ${ctx.attempt})`);
  await new Promise((r) => setTimeout(r, 1_000));
});

// Wire SIGTERM/SIGINT to graceful shutdown.
const controller = new AbortController();
for (const sig of ['SIGTERM', 'SIGINT'] as const) {
  process.on(sig, () => controller.abort());
}

await runner.run(controller.signal);
```

## Features

- **ESM-first**, native `fetch` + `AbortController` — no `axios`, no `node-fetch` shim.
- **Two handler styles**: delegate (`runner.handle('key', fn)`) and a default catch-all (`runner.setDefaultHandler(fn)`).
- **Server-side cancellation** — `PollResponse.cancel` aborts the handler's `ctx.signal`.
- **Streaming `LogWriter`** — bounded queue with backpressure, batch by count (32) / by time (200 ms) / max 100 events per POST, **drain-before-ack**.
- **Self-registration** — `runner.handle('key', fn, { schedule: '5m' })` calls `POST /v1/jobs/register` once at startup (DSL precedence applies server-side).
- **Producer trigger client** — `createTriggerClient(...)` fires jobs on demand via `POST /v1/trigger` with its own `jobs:trigger` credentials, `idempotency_key` dedup, and `429` backpressure surfaced as `QueueOverflowError`. See [Trigger a job on demand](#trigger-a-job-on-demand-producer).
- **Auth precedence** — `Authorization: ApiKey {key}` if set, else `Bearer {token}`.
- **Persistent runner-id** — env var → state file → generated `{prefix}-{hex8}`, persisted under `$XDG_STATE_HOME/croniq-runner` (Linux/macOS) or `%LOCALAPPDATA%\croniq-runner` (Windows).
- **OpenTelemetry-ready** — bring your own `@opentelemetry/sdk-node` setup; the SDK plays nicely with the standard `@opentelemetry/api` no-op fallback.

## Capabilities vs Tags

Same rule as the .NET / Rust SDKs — **don't put implementation details into capabilities**. Capabilities drive routing (`require` / `prefer` in the Croniqfile); tags are filter-only.

| Good capability                              | Bad capability                |
| -------------------------------------------- | ----------------------------- |
| `billing`, `reporting`, `gpu`, `sandboxed`   | `nodejs`, `typescript`, `linux-x64` |

If your runner is Node-based, put that into **tags** (`lang=typescript`, `platform=linux-x64`) so a future Go or Python runner with the same business capabilities can take over without rewriting Croniqfile entries.

## Handler API

The handler receives an `ExecutionContext`:

```ts
import type { ExecutionContext } from '@nuetzliches/croniq-runner';

runner.handle('billing:invoice', async (ctx: ExecutionContext) => {
  const meta = ctx.metadata as { customer_id: string };

  // Structured logger pre-scoped with execution_id, job_key, runner_id, attempt.
  ctx.logger.info('generating invoice', { customer_id: meta.customer_id });

  // Streaming events visible in the Croniq UI execution log:
  for await (const line of generateInvoice(meta.customer_id, ctx.signal)) {
    await ctx.logWriter.write('info', line);
  }

  // ctx.signal aborts when the server cancels OR the runner drain-timeout fires.
  if (ctx.signal.aborted) return;
});
```

`ctx.metadata` is the raw server-provided JSON — cast to your expected shape. Field names are the original snake_case sent by the server.

## Streaming logs

```ts
runner.handle('export:report', async (ctx) => {
  const writer = ctx.logWriter;
  for (let i = 0; i < 100; i++) {
    await writer.write('info', `processing row ${i}`, { row: String(i) });
  }
  // No explicit flush() needed — the runner drains the writer before ack.
  // Call writer.flush() if you need a synchronization barrier mid-handler.
});
```

The writer batches by count and time and survives transient POST failures (drops the batch, logs a warning, keeps going). On per-execution shutdown the runner drains queued events with a configurable timeout (`logWriter.shutdownTimeoutMs`, default 5 s) — late events never arrive after the ack.

## Self-register a schedule

```ts
runner.handle(
  'reports:daily',
  async (ctx) => { /* … */ },
  { schedule: '1h', timeout: '10m', description: 'rolling 24-hour digest' },
);
```

On startup the SDK calls `POST /v1/jobs/register`. If a Croniqfile (DSL) entry with the same `job_key` exists, the server returns `status=skipped_dsl_precedence` and the SDK logs an info message — your runner still polls and executes the job normally, just driven by the DSL schedule rather than the runner-registered one.

## Trigger a job on demand (producer)

The runner above is the **consumer** side. The **producer** side — firing a job _immediately_, e.g. in response to an application event — is a separate, first-class client wrapping `POST /v1/trigger`. It is independent of the runner: triggering needs the `jobs:trigger` (or `admin`) scope, which runner poll keys typically don't carry, so the trigger client takes **its own** credentials rather than reusing a runner's. (Parity with the .NET SDK's `ICroniqTriggerClient`.)

```ts
import { createTriggerClient } from '@nuetzliches/croniq-runner';

const client = createTriggerClient({
  serverUrl: 'http://localhost:4000',
  apiKey: process.env.CRONIQ_TRIGGER_KEY, // jobs:trigger scope — NOT a runner poll key
});

const { executionId, queued, deduplicated } = await client.trigger('billing:invoice-generate', {
  metadata: { invoice_id: 'inv_42' },
  require: ['billing'],
  prefer: ['eu-central'],
  timeout: '10m',
  idempotencyKey: 'evt-2026-07-14-001', // optional server-side dedup
});
```

The same registered handler serves both its Croniqfile schedule (safety-net / reconcile floor) and near-real-time, event-driven fires — one execution and observability path, no second code path.

- **Unset optionals are omitted** from the JSON body — a producer never sends `metadata` / `require` / `prefer` / `timeout` / `idempotency_key` the caller didn't supply.
- **Idempotency.** Pass `idempotencyKey` so at-least-once producers (event redelivery, retries, concurrent publishers) coalesce onto one execution. The result's `deduplicated` is `true` when the server returned an existing execution; servers without idempotency support omit the flag and it defaults to `false`.
- **Backpressure.** A `429` (per-job queue-overflow cap) throws `QueueOverflowError` — a subclass of `HttpError` carrying `retryAfterMs` when the server sends `Retry-After` — so a batching / retrying producer can back off instead of dropping work. Any other non-2xx throws `HttpError`.

| Option             | Default        | Description                                          |
| ------------------ | -------------- | ---------------------------------------------------- |
| `serverUrl`        | _(required)_   | Croniq server base URL.                              |
| `apiKey`           | —              | `Authorization: ApiKey {…}`. Precedence over bearer. |
| `bearerToken`      | —              | `Authorization: Bearer {…}`.                         |
| `requestTimeoutMs` | `30_000`       | Per-request timeout.                                 |
| `fetchImpl`        | global `fetch` | Custom `fetch` (testing / proxies).                  |

## Configuration

| Option                | Default                                              | Description                                                              |
| --------------------- | ---------------------------------------------------- | ------------------------------------------------------------------------ |
| `serverUrl`           | _(required)_                                         | Croniq server base URL.                                                  |
| `runnerId`            | resolved                                             | Stable id. Falls back to `RUNNER_ID` env var → state file → generated.   |
| `runnerIdPrefix`      | `"runner"`                                           | Used when generating an id.                                              |
| `runnerDataDir`       | platform default                                     | Where the persistent `runner-id` file lives.                             |
| `apiKey`              | —                                                    | `Authorization: ApiKey {…}`. Takes precedence over `bearerToken`.        |
| `bearerToken`         | —                                                    | `Authorization: Bearer {…}`.                                             |
| `capabilities`        | `[]`                                                 | Routing capabilities.                                                    |
| `tags`                | `[]`                                                 | Free-form `key=value` tags. Filter-only.                                 |
| `maxInflight`         | `5`                                                  | Max concurrent in-flight executions.                                     |
| `pollTimeoutMs`       | `35_000`                                             | Per-request timeout on the long-poll.                                    |
| `renewIntervalMs`     | `15_000`                                             | Heartbeat cadence for in-flight executions.                              |
| `drainTimeoutMs`      | `30_000`                                             | Graceful shutdown budget before hard-cancel.                             |
| `pollRetryDelayMs`    | `5_000`                                              | Back-off after a failed poll.                                            |
| `capacityBackoffMs`   | `500`                                                | Idle wait when at `maxInflight`.                                         |
| `logWriter`           | see below                                            | Streaming log-writer tunables.                                           |

LogWriter sub-options: `channelCapacity` (256), `batchSizeThreshold` (32), `batchTimeThresholdMs` (200), `maxBatchPerPost` (100), `shutdownTimeoutMs` (5_000).

## Wire-protocol conformance

Validated against the shared, language-neutral suite at [`sdks/conformance/`](../conformance/) — the runner (consumer) cases in [`cases/`](../conformance/cases), plus the producer (trigger) cases in [`cases-trigger/`](../conformance/cases-trigger) ([#287](https://github.com/nuetzliches/croniq/issues/287)) which the binding's trigger runner picks up automatically once present. The TypeScript binding lives at [`sdks/conformance/bindings/typescript/`](../conformance/bindings/typescript).

```sh
cd sdks/conformance/bindings/typescript
npm install
npm test
```

## Releasing

Publishing is fully automated by [`typescript-sdk-release.yml`](../../.github/workflows/typescript-sdk-release.yml). To ship a new version:

1. Bump `version` in [`package.json`](package.json) and update the [CHANGELOG](CHANGELOG.md).
2. Merge to `main`.
3. Tag the commit: `git tag ts-sdk-v0.2.0 && git push --tags`.

The workflow re-runs typecheck, lint, unit tests, build, and the full conformance suite against the freshly built artefact, then `npm publish --provenance --access public`. Pre-release tags (anything with a `-`, e.g. `ts-sdk-v0.2.0-rc.1`) publish under the `next` npm dist-tag so `npm install @nuetzliches/croniq-runner` keeps pointing at the latest stable.

Prerequisite: a repo admin must set the `NPM_TOKEN` secret (an npm automation or granular token with publish rights to the `@croniq` scope) once.

## Compatibility matrix

| SDK Version | Node.js     | Croniq Server (min) | Croniq Server (max tested) |
| ----------- | ----------- | ------------------- | -------------------------- |
| 0.1.x       | ≥ 18.0.0    | 0.14.0              | 0.14.0                     |

## License

Dual-licensed under MIT OR Apache-2.0. See [LICENSE-MIT](../../LICENSE-MIT) and [LICENSE-APACHE](../../LICENSE-APACHE).
