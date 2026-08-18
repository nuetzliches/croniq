import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';

import { describe, expect, it } from 'vitest';

import { HttpError, RunnerOwnershipDeniedError, isOwnershipDenied } from '../src/client.js';
import { CroniqRunner } from '../src/runner.js';

/**
 * A 403 on the poll endpoint is the ownership refusal from #436: the
 * credential is bound to a different `runner_id`. It is permanent, so the
 * runner must stop on the first one rather than retrying on the poll
 * interval — the counterpart to the 409 story, which retries (issue #437).
 */
describe('poll 403 is fatal', () => {
  it('classifies only a 403 as an ownership refusal', () => {
    expect(isOwnershipDenied(new HttpError(403, 'Forbidden', '/v1/work/poll', ''))).toBe(true);
    for (const status of [404, 409, 500, 503]) {
      expect(isOwnershipDenied(new HttpError(status, 'x', '/v1/work/poll', ''))).toBe(false);
    }
    expect(isOwnershipDenied(new Error('network'))).toBe(false);
  });

  it('names the runner_id and the remedy in the error', () => {
    const err = new RunnerOwnershipDeniedError('runner-42');
    expect(err.runnerId).toBe('runner-42');
    expect(err.message).toContain('runner-42');
    expect(err.message).toContain('DELETE /v1/runners/{id}');
  });

  it('rejects run() after a single poll instead of retrying', async () => {
    let polls = 0;
    const server = createServer((req, res) => {
      if (req.url === '/v1/work/poll') polls += 1;
      res.writeHead(403, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'runner_id is bound to another credential' }));
    });
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
    const { port } = server.address() as AddressInfo;

    try {
      const runner = new CroniqRunner({
        serverUrl: `http://127.0.0.1:${port}`,
        runnerId: 'runner-denied',
        apiKey: 'croniq_testkey',
        pollTimeoutMs: 500,
        pollRetryDelayMs: 50,
        drainTimeoutMs: 500,
      });

      await expect(runner.run()).rejects.toBeInstanceOf(RunnerOwnershipDeniedError);
      expect(polls).toBe(1);
    } finally {
      await new Promise<void>((r) => server.close(() => r()));
    }
  });
});
