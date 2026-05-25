import { describe, expect, it } from 'vitest';

import { noopLogger } from '../src/logger.js';
import { resolveOptions } from '../src/options.js';

describe('resolveOptions', () => {
  it('fills sensible defaults', () => {
    const r = resolveOptions({ serverUrl: 'http://localhost:4000' }, noopLogger);
    expect(r.maxInflight).toBe(5);
    expect(r.pollTimeoutMs).toBe(35_000);
    expect(r.renewIntervalMs).toBe(15_000);
    expect(r.drainTimeoutMs).toBe(30_000);
    expect(r.pollRetryDelayMs).toBe(5_000);
    expect(r.capacityBackoffMs).toBe(500);
    expect(r.capabilities).toEqual([]);
    expect(r.tags).toEqual([]);
    expect(r.runnerIdPrefix).toBe('runner');
  });

  it('trims trailing slashes from serverUrl', () => {
    expect(resolveOptions({ serverUrl: 'http://localhost:4000//' }, noopLogger).serverUrl).toBe(
      'http://localhost:4000',
    );
  });

  it('rejects missing serverUrl', () => {
    expect(() => resolveOptions({ serverUrl: '' }, noopLogger)).toThrow(TypeError);
  });

  it('rejects non-URL serverUrl', () => {
    expect(() => resolveOptions({ serverUrl: 'not a url' }, noopLogger)).toThrow(TypeError);
  });

  it('rejects out-of-range maxInflight', () => {
    expect(() => resolveOptions({ serverUrl: 'http://x', maxInflight: 0 }, noopLogger)).toThrow(RangeError);
    expect(() => resolveOptions({ serverUrl: 'http://x', maxInflight: 1025 }, noopLogger)).toThrow(RangeError);
    expect(() => resolveOptions({ serverUrl: 'http://x', maxInflight: 1.5 }, noopLogger)).toThrow(RangeError);
  });

  it('applies LogWriter defaults', () => {
    const r = resolveOptions({ serverUrl: 'http://x' }, noopLogger);
    expect(r.logWriter).toEqual({
      channelCapacity: 256,
      batchSizeThreshold: 32,
      batchTimeThresholdMs: 200,
      maxBatchPerPost: 100,
      shutdownTimeoutMs: 5_000,
    });
  });

  it('respects LogWriter overrides', () => {
    const r = resolveOptions(
      { serverUrl: 'http://x', logWriter: { batchSizeThreshold: 8, batchTimeThresholdMs: 50 } },
      noopLogger,
    );
    expect(r.logWriter.batchSizeThreshold).toBe(8);
    expect(r.logWriter.batchTimeThresholdMs).toBe(50);
    // Untouched defaults still apply.
    expect(r.logWriter.maxBatchPerPost).toBe(100);
  });

  it('clones capabilities and tags so caller-side mutation is isolated', () => {
    const caps = ['a'];
    const tags = ['x=y'];
    const r = resolveOptions({ serverUrl: 'http://x', capabilities: caps, tags }, noopLogger);
    caps.push('b');
    tags.push('p=q');
    expect(r.capabilities).toEqual(['a']);
    expect(r.tags).toEqual(['x=y']);
  });
});
