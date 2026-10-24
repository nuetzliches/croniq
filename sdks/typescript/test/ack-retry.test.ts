// Review 2026-10-24: K-1 (ack retry), K-4 (failure ack after the drain
// timeout), K-9 (poll retry jitter), plus the dispatch-rejection guard.

import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';

import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { ackBudgetMs, ackRetry, isRetryableAckError, jittered } from '../src/backoff.js';
import { HttpError } from '../src/client.js';
import { sleep } from '../src/deferred.js';
import { noopLogger, type Logger } from '../src/logger.js';
import type { JobHandler } from '../src/handler.js';
import { CroniqRunner } from '../src/runner.js';

type AckReply = number | 'destroy' | 'hang';

interface Harness {
  url: string;
  acks: Array<Record<string, unknown>>;
  close: () => Promise<void>;
}

/**
 * A mock server that hands out one execution on the first poll and answers the
 * N-th ack with `ackReplies[N]` (the last entry repeats). `destroy` resets the
 * connection, `hang` never answers.
 */
async function startServer(jobKey: string, ackReplies: AckReply[]): Promise<Harness> {
  const acks: Array<Record<string, unknown>> = [];
  let polls = 0;
  const hung: ServerResponse[] = [];
  const server = createServer((req: IncomingMessage, res: ServerResponse) => {
    let raw = '';
    req.on('data', (c: Buffer) => (raw += c.toString()));
    req.on('end', () => {
      if (req.url === '/v1/work/poll') {
        polls += 1;
        const work =
          polls === 1
            ? [{ execution_id: 'exec-1', job_key: jobKey, fire_at: '2026-10-24T10:00:00Z', attempt: 1, metadata: {}, timeout: '5m' }]
            : [];
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ work, cancel: [] }));
        return;
      }
      if (req.url === '/v1/work/ack') {
        acks.push(JSON.parse(raw) as Record<string, unknown>);
        const reply = ackReplies[Math.min(acks.length - 1, ackReplies.length - 1)]!;
        if (reply === 'destroy') {
          req.socket.destroy();
          return;
        }
        if (reply === 'hang') {
          hung.push(res);
          return;
        }
        res.writeHead(reply, { 'content-type': 'application/json' });
        res.end('{}');
        return;
      }
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end('{}');
    });
  });
  await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
  const { port } = server.address() as AddressInfo;
  return {
    url: `http://127.0.0.1:${port}`,
    acks,
    close: async () => {
      for (const res of hung) res.destroy();
      server.closeAllConnections();
      await new Promise<void>((r) => server.close(() => r()));
    },
  };
}

function makeRunner(url: string, logger: Logger = noopLogger, extra: Record<string, number> = {}): CroniqRunner {
  return new CroniqRunner({
    serverUrl: url,
    runnerId: 'runner-ack',
    apiKey: 'croniq_testkey',
    maxInflight: 1,
    pollTimeoutMs: 500,
    pollRetryDelayMs: 50,
    capacityBackoffMs: 20,
    requestTimeoutMs: 300,
    drainTimeoutMs: 1_000,
    logger,
    ...extra,
  });
}

/** Runs the runner until `done()` holds (or `maxMs` passes), then shuts it down. */
async function runUntil(runner: CroniqRunner, done: () => boolean, maxMs: number, settleMs = 0): Promise<void> {
  const ac = new AbortController();
  const run = runner.run(ac.signal);
  const deadline = Date.now() + maxMs;
  while (!done() && Date.now() < deadline) await sleep(10);
  if (settleMs > 0) await sleep(settleMs);
  ac.abort();
  await run;
}

const noop: JobHandler = async () => {};

describe('ack retry (review 2026-10-24, K-1)', () => {
  const original = ackRetry.delaysMs;
  let harness: Harness | undefined;

  beforeEach(() => {
    ackRetry.delaysMs = [30, 60];
  });
  afterEach(async () => {
    ackRetry.delaysMs = original;
    await harness?.close();
    harness = undefined;
  });

  it('retries a 5xx and stops after the 200', async () => {
    harness = await startServer('job', [503, 200]);
    const runner = makeRunner(harness.url).handle('job', noop);
    await runUntil(runner, () => harness!.acks.length >= 2, 3_000, 200);
    expect(harness.acks).toHaveLength(2);
    expect(harness.acks[1]).toMatchObject({ execution_id: 'exec-1', status: 'success' });
  });

  it('does not retry a 4xx', async () => {
    harness = await startServer('job', [409]);
    const runner = makeRunner(harness.url).handle('job', noop);
    await runUntil(runner, () => harness!.acks.length >= 1, 3_000, 300);
    expect(harness.acks).toHaveLength(1);
  });

  it('gives up after three attempts', async () => {
    harness = await startServer('job', [503]);
    const runner = makeRunner(harness.url).handle('job', noop);
    await runUntil(runner, () => harness!.acks.length >= 3, 3_000, 300);
    expect(harness.acks).toHaveLength(3);
  });

  it('retries a reset connection and a timed-out request', async () => {
    harness = await startServer('job', ['destroy', 'hang', 200]);
    const runner = makeRunner(harness.url).handle('job', noop);
    await runUntil(runner, () => harness!.acks.length >= 3, 3_000, 200);
    expect(harness.acks).toHaveLength(3);
  });

  it('keeps the execution in flight until the ack is done', async () => {
    ackRetry.delaysMs = [300, 300];
    harness = await startServer('job', [503, 200]);
    const runner = makeRunner(harness.url).handle('job', noop);
    const ac = new AbortController();
    const run = runner.run(ac.signal);
    while (harness.acks.length < 1) await sleep(10);
    // Between the failed first attempt and its retry.
    await sleep(100);
    expect(runner.inflight).toEqual(['exec-1']);
    while (harness.acks.length < 2) await sleep(10);
    ac.abort();
    await run;
    expect(runner.inflight).toEqual([]);
  });

  it('classifies only transport errors and 5xx as retryable', () => {
    expect(isRetryableAckError(new HttpError(500, 'x', '/v1/work/ack', ''))).toBe(true);
    expect(isRetryableAckError(new HttpError(503, 'x', '/v1/work/ack', ''))).toBe(true);
    for (const status of [400, 403, 404, 409, 429]) {
      expect(isRetryableAckError(new HttpError(status, 'x', '/v1/work/ack', ''))).toBe(false);
    }
    expect(isRetryableAckError(new TypeError('fetch failed'))).toBe(true);
  });

  it('bounds one ack by every attempt timing out plus the pauses', () => {
    ackRetry.delaysMs = [1_000, 2_000];
    expect(ackBudgetMs(300)).toBe(3 * 300 + 3_000);
  });
});

describe('drain hard-cancel (review 2026-10-24, K-4)', () => {
  const original = ackRetry.delaysMs;
  let harness: Harness | undefined;

  beforeEach(() => {
    ackRetry.delaysMs = [300, 300];
  });
  afterEach(async () => {
    ackRetry.delaysMs = original;
    await harness?.close();
    harness = undefined;
  });

  it('delivers the failure ack, retry included, before run() returns', async () => {
    harness = await startServer('stuck', [503, 200]);
    // Honours its signal only: without the hard cancel it would never return.
    const stuck: JobHandler = (ctx) =>
      new Promise((_resolve, reject) => {
        ctx.signal.addEventListener('abort', () => reject(new Error('aborted')), { once: true });
      });
    const runner = makeRunner(harness.url, noopLogger, { drainTimeoutMs: 100 }).handle('stuck', stuck);
    const ac = new AbortController();
    const run = runner.run(ac.signal);
    while (runner.inflight.length === 0) await sleep(10);
    ac.abort();
    await run;
    // The old fixed 1 s grace would also have covered this retry; what matters
    // is that run() did not return before the second ack was sent.
    expect(harness.acks).toHaveLength(2);
    expect(harness.acks[1]).toMatchObject({ execution_id: 'exec-1', status: 'failure', error: 'runner draining' });
  });

  it('waits past the old 1 s grace for a slow ack', async () => {
    ackRetry.delaysMs = [1_500, 1_500];
    harness = await startServer('stuck', [503, 200]);
    const stuck: JobHandler = (ctx) =>
      new Promise((_resolve, reject) => {
        ctx.signal.addEventListener('abort', () => reject(new Error('aborted')), { once: true });
      });
    const runner = makeRunner(harness.url, noopLogger, { drainTimeoutMs: 100 }).handle('stuck', stuck);
    const ac = new AbortController();
    const run = runner.run(ac.signal);
    while (runner.inflight.length === 0) await sleep(10);
    ac.abort();
    await run;
    expect(harness.acks).toHaveLength(2);
    expect(runner.inflight).toEqual([]);
  });
});

describe('dispatch rejection guard', () => {
  let harness: Harness | undefined;
  afterEach(async () => {
    await harness?.close();
    harness = undefined;
  });

  it('frees the slot and swallows the rejection when dispatch throws', async () => {
    harness = await startServer('boom', [200]);
    const errors: string[] = [];
    // warn throws, so the dispatcher's own handler-error path rejects before
    // it acks; the runner must not leak that as an unhandled rejection.
    const logger: Logger = {
      ...noopLogger,
      warn: () => {
        throw new Error('logger is broken');
      },
      error: (message: string) => {
        errors.push(message);
      },
    };
    const unhandled: unknown[] = [];
    const onUnhandled = (reason: unknown): void => {
      unhandled.push(reason);
    };
    process.on('unhandledRejection', onUnhandled);
    try {
      const runner = makeRunner(harness.url, logger).handle('boom', async () => {
        throw new Error('handler failed');
      });
      await runUntil(runner, () => errors.length > 0, 2_000, 50);
      expect(errors).toContain('execution dispatch failed unexpectedly');
      expect(runner.inflight).toEqual([]);
      expect(unhandled).toEqual([]);
    } finally {
      process.off('unhandledRejection', onUnhandled);
    }
  });
});

describe('poll retry jitter (review 2026-10-24, K-9)', () => {
  it('stays within ±20 % of the configured delay', () => {
    expect(jittered(1_000, () => 0)).toBe(800);
    expect(jittered(1_000, () => 0.5)).toBe(1_000);
    expect(jittered(1_000, () => 0.999_999)).toBe(1_200);
    for (let i = 0; i < 1_000; i += 1) {
      const d = jittered(5_000);
      expect(d).toBeGreaterThanOrEqual(4_000);
      expect(d).toBeLessThanOrEqual(6_000);
    }
  });

  it('actually varies', () => {
    const seen = new Set(Array.from({ length: 50 }, () => jittered(5_000)));
    expect(seen.size).toBeGreaterThan(1);
  });
});
