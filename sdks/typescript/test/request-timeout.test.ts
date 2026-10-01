// Issue #792: the ack carried no timeout and a signal nothing ever aborted, so
// an ack the server never answered never settled — and the execution stayed in
// the in-flight set every poll reports, keeping its lease renewed forever.
// Every non-poll request is bounded by `requestTimeoutMs` now.

import { describe, expect, it } from 'vitest';

import { CroniqClient } from '../src/client.js';
import { noopLogger } from '../src/logger.js';
import { resolveOptions } from '../src/options.js';

/** A `fetch` that never answers, like a half-open socket; it only honours abort. */
const neverAnswers: typeof fetch = (_input, init) =>
  new Promise<Response>((_resolve, reject) => {
    const signal = init?.signal;
    signal?.addEventListener('abort', () => reject(signal.reason ?? new Error('aborted')));
  });

function silentClient(requestTimeoutMs: number): CroniqClient {
  return new CroniqClient({
    baseUrl: 'http://localhost:4000',
    fetchImpl: neverAnswers,
    requestTimeoutMs,
  });
}

describe('request timeout (issue #792)', () => {
  it('gives up on an ack the server never answers', async () => {
    const client = silentClient(100);
    const neverAborted = new AbortController().signal;

    const started = Date.now();
    await expect(
      client.ack(
        { runner_id: 'r1', execution_id: 'e1', status: 'success', duration_ms: 1, attempt: 1 },
        neverAborted,
      ),
    ).rejects.toBeDefined();
    expect(Date.now() - started).toBeLessThan(5_000);
  });

  it('bounds renew and event pushes too', async () => {
    const client = silentClient(100);
    const neverAborted = new AbortController().signal;

    await expect(client.renew({ runner_id: 'r1', execution_id: 'e1' }, neverAborted)).rejects.toBeDefined();
    await expect(client.pushEvents('e1', [{ level: 'info', message: 'line' }], neverAborted)).rejects.toBeDefined();
  });

  it('defaults to 30 seconds', () => {
    const resolved = resolveOptions({ serverUrl: 'http://localhost:4000' }, noopLogger);
    expect(resolved.requestTimeoutMs).toBe(30_000);
  });
});
