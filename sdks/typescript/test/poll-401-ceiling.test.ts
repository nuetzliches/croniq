import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';

import { describe, expect, it } from 'vitest';

import { AuthFailedError, HttpError, isUnauthorized } from '../src/client.js';
import { CroniqRunner } from '../src/runner.js';

/**
 * A 401 on the poll endpoint means the API key was rejected. The client reads
 * its credential once and never re-reads it, so retrying presents the same
 * dead key forever — before issue #473 a 401 fell into the generic transient
 * bucket and the runner polled indefinitely: up, healthy-looking, doing
 * nothing, and never exiting non-zero, so nothing ever restarted it.
 *
 * Unlike the 403 of `poll-403-fatal`, the first one is not fatal. Rotation
 * hands over by installing the new key and giving the old one an expiry
 * (server issue #471), so a narrow race around that handover must not kill a
 * healthy runner.
 */
describe('poll 401 auth ceiling', () => {
  it('classifies only a 401 as an auth failure', () => {
    expect(isUnauthorized(new HttpError(401, 'Unauthorized', '/v1/work/poll', ''))).toBe(true);
    for (const status of [403, 404, 409, 500, 503]) {
      expect(isUnauthorized(new HttpError(status, 'x', '/v1/work/poll', ''))).toBe(false);
    }
    expect(isUnauthorized(new Error('network'))).toBe(false);
  });

  it('names the streak and the remedy in the error', () => {
    const err = new AuthFailedError(3);
    expect(err.consecutiveCount).toBe(3);
    expect(err.message).toContain('revoked');
    expect(err.message).toContain('Restart the runner');
  });

  it('rejects run() once the streak exhausts the ceiling', async () => {
    let polls = 0;
    const server = createServer((req, res) => {
      if (req.url === '/v1/work/poll') polls += 1;
      res.writeHead(401, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'unauthorized' }));
    });
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
    const { port } = server.address() as AddressInfo;

    try {
      const runner = new CroniqRunner({
        serverUrl: `http://127.0.0.1:${port}`,
        runnerId: 'runner-revoked',
        apiKey: 'croniq_testkey',
        pollTimeoutMs: 500,
        pollRetryDelayMs: 20,
        drainTimeoutMs: 500,
        maxConsecutiveAuthFailures: 3,
      });

      await expect(runner.run()).rejects.toBeInstanceOf(AuthFailedError);
      expect(polls).toBe(3);
    } finally {
      await new Promise<void>((r) => server.close(() => r()));
    }
  });

  it('survives a single 401 and keeps polling', async () => {
    // The rotation-handover case: one rejection, then the new key works.
    const statuses = [401, 200];
    let polls = 0;
    const server = createServer((req, res) => {
      if (req.url !== '/v1/work/poll') {
        res.writeHead(404);
        res.end('{}');
        return;
      }
      const status = statuses[Math.min(polls, statuses.length - 1)]!;
      polls += 1;
      res.writeHead(status, { 'content-type': 'application/json' });
      res.end(status === 200 ? JSON.stringify({ work: [], cancel: [] }) : JSON.stringify({ error: 'unauthorized' }));
    });
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
    const { port } = server.address() as AddressInfo;

    try {
      const runner = new CroniqRunner({
        serverUrl: `http://127.0.0.1:${port}`,
        runnerId: 'runner-rotating',
        apiKey: 'croniq_testkey',
        pollTimeoutMs: 500,
        pollRetryDelayMs: 20,
        drainTimeoutMs: 500,
        maxConsecutiveAuthFailures: 3,
      });

      const ac = new AbortController();
      const run = runner.run(ac.signal);
      while (polls < 2) await new Promise((r) => setTimeout(r, 10));
      ac.abort();
      await run;
      expect(polls).toBeGreaterThanOrEqual(2);
    } finally {
      await new Promise<void>((r) => server.close(() => r()));
    }
  });

  it('resets the streak on a non-401 failure', async () => {
    // A 500 says nothing about whether the credential is valid, so an
    // unlucky mix of failures must not add up to a fatal error.
    const statuses = [401, 500, 401, 200];
    let polls = 0;
    const server = createServer((req, res) => {
      if (req.url !== '/v1/work/poll') {
        res.writeHead(404);
        res.end('{}');
        return;
      }
      const status = statuses[Math.min(polls, statuses.length - 1)]!;
      polls += 1;
      res.writeHead(status, { 'content-type': 'application/json' });
      res.end(status === 200 ? JSON.stringify({ work: [], cancel: [] }) : JSON.stringify({ error: 'nope' }));
    });
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
    const { port } = server.address() as AddressInfo;

    try {
      const runner = new CroniqRunner({
        serverUrl: `http://127.0.0.1:${port}`,
        runnerId: 'runner-flaky-auth',
        apiKey: 'croniq_testkey',
        pollTimeoutMs: 500,
        pollRetryDelayMs: 20,
        drainTimeoutMs: 500,
        maxConsecutiveAuthFailures: 2,
      });

      const ac = new AbortController();
      const run = runner.run(ac.signal);
      while (polls < 4) await new Promise((r) => setTimeout(r, 10));
      ac.abort();
      await run;
      expect(polls).toBeGreaterThanOrEqual(4);
    } finally {
      await new Promise<void>((r) => server.close(() => r()));
    }
  });

  it('refuses a ceiling outside [1, 100]', () => {
    expect(
      () =>
        new CroniqRunner({
          serverUrl: 'http://127.0.0.1:4000',
          maxConsecutiveAuthFailures: 0,
        }),
    ).toThrow(RangeError);
  });
});
