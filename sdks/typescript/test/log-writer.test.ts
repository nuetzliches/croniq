import { describe, expect, it, vi } from 'vitest';

import type { CroniqClient } from '../src/client.js';
import { LogEnrichment } from '../src/enrichment.js';
import { noopLogger } from '../src/logger.js';
import { StreamingLogWriter } from '../src/log-writer.js';
import type { ResolvedLogWriterOptions } from '../src/options.js';
import type { WorkEvent } from '../src/protocol.js';

function fakeClient(): { client: CroniqClient; batches: WorkEvent[][] } {
  const batches: WorkEvent[][] = [];
  const client = {
    pushEvents: vi.fn(async (_execId: string, events: WorkEvent[]) => {
      batches.push(events);
    }),
  } as unknown as CroniqClient;
  return { client, batches };
}

const baseOptions: ResolvedLogWriterOptions = {
  channelCapacity: 256,
  batchSizeThreshold: 32,
  batchTimeThresholdMs: 50,
  maxBatchPerPost: 10,
  shutdownTimeoutMs: 5_000,
};

describe('StreamingLogWriter', () => {
  it('drains queued events on dispose (flush-before-ack)', async () => {
    const { client, batches } = fakeClient();
    const w = new StreamingLogWriter(client, 'exec-1', new LogEnrichment('j', 'r', []), baseOptions, noopLogger);

    for (let i = 0; i < 25; i++) await w.write('info', `line ${i}`);
    await w.dispose();

    const total = batches.reduce((sum, b) => sum + b.length, 0);
    expect(total).toBe(25);
    // Enrichment runs at POST time
    expect(batches[0]![0]!.fields?.job_key).toBe('j');
    expect(batches[0]![0]!.fields?.runner_id).toBe('r');
  });

  it('chunks each POST by maxBatchPerPost', async () => {
    const { client, batches } = fakeClient();
    const opts: ResolvedLogWriterOptions = { ...baseOptions, maxBatchPerPost: 4 };
    const w = new StreamingLogWriter(client, 'exec-1', new LogEnrichment('j', 'r', []), opts, noopLogger);

    for (let i = 0; i < 10; i++) await w.write('info', `line ${i}`);
    await w.dispose();

    // 10 events into chunks of max 4 → 4 + 4 + 2 (order/split may vary if the
    // flusher races but the per-POST cap is the invariant we check).
    for (const batch of batches) expect(batch.length).toBeLessThanOrEqual(4);
    expect(batches.reduce((s, b) => s + b.length, 0)).toBe(10);
  });

  it('flush() waits until the queue is drained', async () => {
    const { client, batches } = fakeClient();
    const w = new StreamingLogWriter(client, 'exec-1', new LogEnrichment('j', 'r', []), baseOptions, noopLogger);

    await w.write('info', 'before-flush');
    await w.flush();
    expect(batches.reduce((s, b) => s + b.length, 0)).toBe(1);

    await w.write('info', 'after-flush');
    await w.dispose();
    expect(batches.reduce((s, b) => s + b.length, 0)).toBe(2);
  });

  it('flushes by time threshold even when batch-size is not reached', async () => {
    const { client, batches } = fakeClient();
    const opts: ResolvedLogWriterOptions = { ...baseOptions, batchTimeThresholdMs: 30, batchSizeThreshold: 1000 };
    const w = new StreamingLogWriter(client, 'exec-1', new LogEnrichment('j', 'r', []), opts, noopLogger);

    await w.write('info', 'lonely');
    // Wait enough wall-clock for at least one time-threshold tick.
    await new Promise((r) => setTimeout(r, 80));
    expect(batches.reduce((s, b) => s + b.length, 0)).toBe(1);
    await w.dispose();
  });

  it('drops batches on POST failure without crashing the writer', async () => {
    let calls = 0;
    const client = {
      pushEvents: vi.fn(async () => {
        if (calls++ === 0) throw new Error('boom');
      }),
    } as unknown as CroniqClient;

    const w = new StreamingLogWriter(client, 'exec-1', new LogEnrichment('j', 'r', []), baseOptions, noopLogger);
    await w.write('info', 'will-fail');
    await w.flush();
    await w.write('info', 'will-succeed');
    await w.dispose();

    expect(calls).toBeGreaterThanOrEqual(2);
  });
});
