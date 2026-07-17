import { describe, expect, it } from 'vitest';

import { parseScheduledFor, parseTimeoutMs } from '../src/duration.js';

describe('parseTimeoutMs', () => {
  it('parses seconds', () => {
    expect(parseTimeoutMs('30s')).toBe(30_000);
  });

  it('parses minutes', () => {
    expect(parseTimeoutMs('5m')).toBe(5 * 60_000);
  });

  it('parses hours', () => {
    expect(parseTimeoutMs('1h')).toBe(3_600_000);
  });

  it('parses days', () => {
    expect(parseTimeoutMs('1d')).toBe(86_400_000);
  });

  it('accepts uppercase and whitespace', () => {
    expect(parseTimeoutMs(' 2M ')).toBe(2 * 60_000);
  });

  it('rejects unknown units', () => {
    expect(parseTimeoutMs('5w')).toBeUndefined();
  });

  it('rejects empty / null / undefined', () => {
    expect(parseTimeoutMs('')).toBeUndefined();
    expect(parseTimeoutMs(null)).toBeUndefined();
    expect(parseTimeoutMs(undefined)).toBeUndefined();
    expect(parseTimeoutMs('x')).toBeUndefined();
  });

  it('rejects malformed numbers', () => {
    expect(parseTimeoutMs('abcm')).toBeUndefined();
  });
});

describe('parseScheduledFor', () => {
  it('parses an RFC 3339 timestamp', () => {
    const d = parseScheduledFor('2026-06-01T06:00:00Z');
    expect(d).toBeInstanceOf(Date);
    expect(d?.toISOString()).toBe('2026-06-01T06:00:00.000Z');
  });

  it('returns null when absent (older server)', () => {
    expect(parseScheduledFor(undefined)).toBeNull();
    expect(parseScheduledFor(null)).toBeNull();
  });

  it('returns null on unparseable input rather than an Invalid Date', () => {
    expect(parseScheduledFor('not-a-date')).toBeNull();
  });
});
