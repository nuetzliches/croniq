import { describe, expect, it } from 'vitest'
import { formatAbsolute, formatDuration, formatRelative, shortId } from './format'

const NOW = Date.parse('2026-09-11T12:00:00Z')
const ago = (ms: number) => new Date(NOW - ms).toISOString()

describe('formatRelative', () => {
  it('collapses the last few seconds', () => {
    expect(formatRelative(ago(0), NOW)).toBe('just now')
    expect(formatRelative(ago(4_000), NOW)).toBe('just now')
    expect(formatRelative(ago(5_000), NOW)).toBe('5 s ago')
  })

  it('steps up through the units', () => {
    expect(formatRelative(ago(59_000), NOW)).toBe('59 s ago')
    expect(formatRelative(ago(60_000), NOW)).toBe('1 min ago')
    expect(formatRelative(ago(90 * 60_000), NOW)).toBe('1 h ago')
    expect(formatRelative(ago(36 * 3_600_000), NOW)).toBe('1 d ago')
    expect(formatRelative(ago(45 * 86_400_000), NOW)).toBe('1 mo ago')
    expect(formatRelative(ago(400 * 86_400_000), NOW)).toBe('1 y ago')
  })

  /**
   * A scheduler's rows carry fire times that are moments *ahead* — a queued
   * run, or two clocks disagreeing by a second. "-1 s ago" is what the naive
   * version prints.
   */
  it('handles instants in the future', () => {
    expect(formatRelative(new Date(NOW + 12_000).toISOString(), NOW)).toBe('in 12 s')
    expect(formatRelative(new Date(NOW + 2 * 60_000).toISOString(), NOW)).toBe('in 2 min')
  })

  it('says nothing useful about nothing', () => {
    expect(formatRelative(null, NOW)).toBe('—')
    expect(formatRelative(undefined, NOW)).toBe('—')
    expect(formatRelative('not a date', NOW)).toBe('—')
  })
})

describe('formatDuration', () => {
  it('keeps milliseconds below a second', () => {
    expect(formatDuration(0)).toBe('0 ms')
    expect(formatDuration(74)).toBe('74 ms')
    expect(formatDuration(999)).toBe('999 ms')
  })

  it('switches to seconds at exactly one second', () => {
    expect(formatDuration(1000)).toBe('1.0 s')
    expect(formatDuration(1921)).toBe('1.9 s')
  })

  it('drops the decimal once seconds get big', () => {
    expect(formatDuration(9_900)).toBe('9.9 s')
    expect(formatDuration(10_000)).toBe('10 s')
    expect(formatDuration(59_000)).toBe('59 s')
  })

  it('switches to minutes and hours', () => {
    expect(formatDuration(60_000)).toBe('1m')
    expect(formatDuration(90_000)).toBe('1m 30s')
    expect(formatDuration(3_600_000)).toBe('1h')
    expect(formatDuration(5_400_000)).toBe('1h 30m')
  })

  it('says nothing useful about nothing', () => {
    expect(formatDuration(null)).toBe('—')
    expect(formatDuration(undefined)).toBe('—')
    expect(formatDuration(-1)).toBe('—')
  })
})

describe('shortId', () => {
  it('takes the first segment of a uuid', () => {
    expect(shortId('f9e28203-5ad4-4ccf-9898-e6f2b5c16071')).toBe('f9e28203')
  })

  it('leaves short values alone', () => {
    expect(shortId('abc')).toBe('abc')
    expect(shortId(null)).toBe('—')
  })
})

describe('formatAbsolute', () => {
  it('is empty rather than "Invalid Date" for junk', () => {
    expect(formatAbsolute(null)).toBe('')
    expect(formatAbsolute('not a date')).toBe('')
    expect(formatAbsolute('2026-09-11T12:00:00Z')).not.toBe('')
  })
})
