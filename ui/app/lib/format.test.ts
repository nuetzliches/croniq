import { describe, expect, it } from 'vitest'
import {
  formatAbsolute,
  formatClockTime,
  formatDuration,
  formatLogLines,
  formatRelative,
  shortId,
  stateLabel,
} from './format'

describe('formatClockTime', () => {
  it('renders the instant in the given zone, winter and summer', () => {
    expect(formatClockTime('2026-01-15T12:34:56.789012Z', 'Europe/Berlin')).toBe('13:34:56.789')
    expect(formatClockTime('2026-07-15T12:34:56.789012Z', 'Europe/Berlin')).toBe('14:34:56.789')
  })

  it('keeps UTC as UTC and pads milliseconds', () => {
    expect(formatClockTime('2026-07-15T00:00:00.005Z', 'UTC')).toBe('00:00:00.005')
  })

  it('crosses midnight with the zone', () => {
    expect(formatClockTime('2026-07-15T23:30:00.000Z', 'Europe/Berlin')).toBe('01:30:00.000')
  })

  it('falls back to the raw slice for an unparseable value', () => {
    const raw = 'not a timestamp at all'
    expect(formatClockTime(raw)).toBe(raw.slice(11, 23))
  })
})

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

describe('stateLabel', () => {
  it('shows a claimed run as running', () => {
    expect(stateLabel('claimed')).toBe('running')
  })

  it('leaves every other state as it is', () => {
    for (const state of ['queued', 'completed', 'failed', 'dead', 'cancelled', 'online']) {
      expect(stateLabel(state)).toBe(state)
    }
  })
})

describe('formatLogLines', () => {
  it('writes one `timestamp LEVEL message` line per event', () => {
    expect(
      formatLogLines([
        { timestamp: '2026-09-30T08:00:01.250Z', level: 'info', message: 'pulling inbox' },
        { timestamp: '2026-09-30T08:00:02.000Z', level: 'error', message: 'timeout' },
      ]),
    ).toBe('2026-09-30T08:00:01.250Z INFO pulling inbox\n2026-09-30T08:00:02.000Z ERROR timeout')
  })

  it('is empty for no events', () => {
    expect(formatLogLines([])).toBe('')
  })
})
