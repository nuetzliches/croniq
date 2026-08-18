import { describe, expect, it, vi } from 'vitest';

import type { CroniqClient } from '../src/client.js';
import { ExecutionDispatcher } from '../src/dispatcher.js';
import { HandlerRegistry } from '../src/handler.js';
import type { Logger } from '../src/logger.js';
import { resolveOptions } from '../src/options.js';
import { noopLogger } from '../src/logger.js';
import type { AckRequest, WorkAssignment } from '../src/protocol.js';

interface Record_ {
  level: string;
  message: string;
  fields: Record<string, unknown>;
}

function recordingLogger(): { logger: Logger; records: Record_[] } {
  const records: Record_[] = [];
  const push = (level: string) => (message: string, fields?: Record<string, unknown>) => {
    records.push({ level, message, fields: fields ?? {} });
  };
  return {
    records,
    logger: {
      trace: push('trace'),
      debug: push('debug'),
      info: push('info'),
      warn: push('warn'),
      error: push('error'),
    },
  };
}

function fakeClient(): { client: CroniqClient; acks: AckRequest[] } {
  const acks: AckRequest[] = [];
  const client = {
    ack: vi.fn(async (payload: AckRequest) => {
      acks.push(payload);
    }),
    renew: vi.fn(async () => {}),
    pushEvents: vi.fn(async () => {}),
  } as unknown as CroniqClient;
  return { client, acks };
}

const JOB_KEY = 'billing:invoice';
const EXECUTION_ID = '6f8c1a2e-4b7d-4a1f-9c3e-2d5b8a0f1e77';

const assignment: WorkAssignment = {
  execution_id: EXECUTION_ID,
  job_key: JOB_KEY,
  fire_at: '2026-05-23T10:00:00Z',
  attempt: 1,
  metadata: {},
  timeout: '1m',
};

describe('ExecutionDispatcher logging', () => {
  it('carries job_key and execution_id as fields, never in the message', async () => {
    const { client, acks } = fakeClient();
    const { logger, records } = recordingLogger();
    const registry = new HandlerRegistry();
    registry.register(JOB_KEY, () => {
      throw new Error('billing service unreachable');
    });

    const dispatcher = new ExecutionDispatcher({
      client,
      registry,
      options: resolveOptions({ serverUrl: 'http://localhost:4000' }, noopLogger),
      runnerId: 'runner-abc',
      runnerTags: [],
      logger,
    });

    await dispatcher.dispatch(assignment, new AbortController(), new AbortController().signal);

    expect(acks).toHaveLength(1);
    expect(acks[0]!.status).toBe('failure');

    const warned = records.find((r) => r.level === 'warn');
    expect(warned).toBeDefined();
    // The identifiers travel as structured fields …
    expect(warned!.fields.job_key).toBe(JOB_KEY);
    expect(warned!.fields.execution_id).toBe(EXECUTION_ID);
    // … and appear nowhere in the message text, which is a constant.
    expect(warned!.message).not.toContain(JOB_KEY);
    expect(warned!.message).not.toContain(EXECUTION_ID);
    for (const record of records) {
      expect(record.message).not.toContain(JOB_KEY);
      expect(record.message).not.toContain(EXECUTION_ID);
    }
  });
});
