import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';

import { describe, expect, it } from 'vitest';

import { HttpError, PollInstanceConflictError, isInstanceConflict } from '../src/client.js';
import { CroniqRunner } from '../src/runner.js';

/**
 * A 409 on the poll endpoint is the fencing refusal from #374: a newer
 * instance has taken this `runner_id` over. A single one is transient — the
 * deposed instance may win the identity back — and conformance case 11 pins
 * that it is retried. A *streak* of them is a duplicate deployment, two
 * processes started with the same fixed `runnerId`, and retrying that forever
 * leaves the misconfiguration behind a warning that scrolls past
 * (issue #134 sub-item 1).
 */
describe('poll 409 conflict ceiling', () => {
  it('classifies only a 409 as an instance conflict', () => {
    expect(isInstanceConflict(new HttpError(409, 'Conflict', '/v1/work/poll', ''))).toBe(true);
    for (const status of [403, 404, 500, 503]) {
      expect(isInstanceConflict(new HttpError(status, 'x', '/v1/work/poll', ''))).toBe(false);
    }
    expect(isInstanceConflict(new Error('network'))).toBe(false);
  });

  it('names the runner_id, the streak and the remedy in the error', () => {
    const err = new PollInstanceConflictError('runner-42', 3);
    expect(err.runnerId).toBe('runner-42');
    expect(err.consecutiveCount).toBe(3);
    expect(err.message).toContain('runner-42');
    expect(err.message).toContain('rotate the runner_id');
  });

  it('rejects run() once the streak exhausts the ceiling', async () => {
    let polls = 0;
    const server = createServer((req, res) => {
      if (req.url === '/v1/work/poll') polls += 1;
      res.writeHead(409, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'runner instance conflict' }));
    });
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
    const { port } = server.address() as AddressInfo;

    try {
      const runner = new CroniqRunner({
        serverUrl: `http://127.0.0.1:${port}`,
        runnerId: 'runner-duplicate',
        apiKey: 'croniq_testkey',
        pollTimeoutMs: 500,
        pollRetryDelayMs: 20,
        drainTimeoutMs: 500,
        maxConsecutivePollConflicts: 3,
      });

      await expect(runner.run()).rejects.toBeInstanceOf(PollInstanceConflictError);
      expect(polls).toBe(3);
    } finally {
      await new Promise<void>((r) => server.close(() => r()));
    }
  });

  it('resets the streak on a non-409 failure', async () => {
    // Only *consecutive* conflicts count: the 500 in between is unrelated to
    // instance ownership, so an unlucky mix of failures must not add up to a
    // fatal error.
    const statuses = [409, 500, 409, 200];
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
        runnerId: 'runner-flaky',
        apiKey: 'croniq_testkey',
        pollTimeoutMs: 500,
        pollRetryDelayMs: 20,
        drainTimeoutMs: 500,
        maxConsecutivePollConflicts: 2,
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
    // 0 would make the runner exit on its very first 409, which reads as a
    // crash-loop rather than a misconfiguration.
    expect(
      () =>
        new CroniqRunner({
          serverUrl: 'http://127.0.0.1:4000',
          maxConsecutivePollConflicts: 0,
        }),
    ).toThrow(RangeError);
  });
});
