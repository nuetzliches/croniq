import { describe, expect, it } from 'vitest'
import {
  ClockOffset,
  MIN_WINDOW,
  SPAN_MS,
  barsOf,
  buildLanes,
  clampRange,
  densityBuckets,
  dragRange,
  formatOffset,
  formatSpan,
  type LiveRun,
} from './live-timeline'

const T = Date.parse('2026-10-01T12:00:00Z')
const iso = (offsetMs: number) => new Date(T + offsetMs).toISOString()

function run(overrides: Partial<LiveRun> & Pick<LiveRun, 'id'>): LiveRun {
  return {
    job_key: 'demo:job',
    state: 'completed',
    attempt: 1,
    runner_id: 'r1',
    fire_at: iso(-10_000),
    claimed_at: iso(-9_000),
    completed_at: iso(-5_000),
    ...overrides,
  }
}

describe('barsOf', () => {
  it('splits a run into its wait and its execution', () => {
    const bars = barsOf(run({ id: 'a' }))
    expect(bars.map((b) => [b.kind, b.start - T, b.end! - T])).toEqual([
      ['wait', -10_000, -9_000],
      ['run', -9_000, -5_000],
    ])
  })

  it('leaves both ends open while a fire is still queued', () => {
    const bars = barsOf(run({ id: 'q', state: 'queued', claimed_at: null, completed_at: null }))
    expect(bars).toHaveLength(1)
    expect(bars[0]).toMatchObject({ kind: 'wait', end: null })
  })

  it('draws no wait when the claim came at the fire', () => {
    const bars = barsOf(run({ id: 'b', claimed_at: iso(-10_000) }))
    expect(bars.map((b) => b.kind)).toEqual(['run'])
  })

  it('ends the wait where a never-claimed run was given up', () => {
    const bars = barsOf(run({ id: 'c', state: 'cancelled', claimed_at: null, completed_at: iso(-2_000) }))
    expect(bars).toHaveLength(1)
    expect(bars[0]!.end! - T).toBe(-2_000)
  })
})

describe('buildLanes', () => {
  it('drops bars that ended before the window', () => {
    const { lanes } = buildLanes([run({ id: 'old', completed_at: iso(-70_000), fire_at: iso(-90_000), claimed_at: iso(-80_000) })], [], T, 60_000)
    expect(lanes).toEqual([])
  })

  it('keeps every lane, however many there are (#828)', () => {
    const schedule = Array.from({ length: 49 }, (_, i) => ({
      job_key: `job:${String(i).padStart(2, '0')}`,
      status: 'active',
      next_fire_at: iso((49 - i) * 1_000),
      overdue: false,
    }))
    const { lanes } = buildLanes([], schedule, T, 60_000)
    expect(lanes).toHaveLength(49)
  })

  it('marks running and overdue lanes', () => {
    const runs = [run({ id: '3', job_key: 'b', state: 'claimed', completed_at: null })]
    const schedule = [{ job_key: 'z', status: 'active', next_fire_at: iso(-60_000), overdue: true }]
    const { lanes } = buildLanes(runs, schedule, T, 60_000)
    expect(lanes.find((l) => l.jobKey === 'b')!.running).toBe(true)
    expect(lanes.find((l) => l.jobKey === 'z')!.overdue).toBe(true)
  })

  it('gives every scheduled job a lane, idle or not', () => {
    const { lanes } = buildLanes(
      [],
      [{ job_key: 'quiet', status: 'active', next_fire_at: iso(22 * 60_000), overdue: false }],
      T,
      60_000,
    )
    expect(lanes.map((l) => [l.jobKey, l.next! - T])).toEqual([['quiet', 22 * 60_000]])
  })

  it('drops the next fire of a job that is not active', () => {
    const { lanes } = buildLanes(
      [],
      [{ job_key: 'held', status: 'paused', next_fire_at: iso(5_000), overdue: false }],
      T,
      60_000,
    )
    expect(lanes[0]).toMatchObject({ jobKey: 'held', next: null, status: 'paused' })
  })
})

describe('buildLanes ordering', () => {
  it('lists lanes by job key by default, so a lane stays put when its job fires', () => {
    const before = [
      { job_key: 'b:poll', status: 'active', next_fire_at: iso(1_000), overdue: false },
      { job_key: 'a:report', status: 'active', next_fire_at: iso(60_000), overdue: false },
    ]
    // b:poll fires: its next fire jumps past a:report's.
    const after = [{ ...before[0]!, next_fire_at: iso(120_000) }, before[1]!]
    const keys = (schedule: typeof before) => buildLanes([], schedule, T, 60_000).lanes.map((l) => l.jobKey)
    expect(keys(before)).toEqual(['a:report', 'b:poll'])
    expect(keys(after)).toEqual(['a:report', 'b:poll'])
  })

  it('lists soonest next fire first on request, jobs without one last and A–Z', () => {
    const schedule = [
      { job_key: 'later', status: 'active', next_fire_at: iso(20 * 60_000), overdue: false },
      { job_key: 'paused', status: 'paused', next_fire_at: iso(1_000), overdue: false },
      { job_key: 'soon', status: 'active', next_fire_at: iso(30_000), overdue: false },
      { job_key: 'manual', status: 'active', next_fire_at: null, overdue: false },
    ]
    const { lanes } = buildLanes([], schedule, T, 60_000, 'next')
    expect(lanes.map((l) => l.jobKey)).toEqual(['soon', 'later', 'manual', 'paused'])
  })
})

describe('ClockOffset', () => {
  it('takes the sample that spent the least time in flight', () => {
    const offset = new ClockOffset()
    // Server is 2 s ahead; the frames took 300 ms and 40 ms to arrive.
    offset.add(iso(2_000), T + 300)
    offset.add(iso(2_000), T + 40)
    expect(offset.value).toBe(1_960)
  })

  it('is zero before the first frame', () => {
    expect(new ClockOffset().value).toBe(0)
  })
})

describe('formatOffset', () => {
  it.each([
    [0, 'now'],
    [-5_000, '−5s'],
    [-60_000, '−1m'],
    [-150_000, '−2m 30s'],
    [10_000, '+10s'],
  ])('%i → %s', (input, expected) => {
    expect(formatOffset(input)).toBe(expected)
  })
})

describe('clampRange', () => {
  it('keeps a range that fits', () => {
    expect(clampRange({ windowMs: 60_000, endOffsetMs: -30_000 })).toEqual({ windowMs: 60_000, endOffsetMs: -30_000 })
  })

  it('never ends after now and never starts before the span', () => {
    expect(clampRange({ windowMs: 60_000, endOffsetMs: 5_000 }).endOffsetMs).toBe(0)
    expect(clampRange({ windowMs: 60_000, endOffsetMs: -SPAN_MS })).toEqual({
      windowMs: 60_000,
      endOffsetMs: 60_000 - SPAN_MS,
    })
  })

  it('bounds the width', () => {
    expect(clampRange({ windowMs: 1, endOffsetMs: 0 }).windowMs).toBe(MIN_WINDOW)
    expect(clampRange({ windowMs: 10 * SPAN_MS, endOffsetMs: 0 }).windowMs).toBe(SPAN_MS)
  })
})

describe('dragRange', () => {
  const live = { windowMs: 60_000, endOffsetMs: 0 }

  it('moves the whole selection into the past, keeping its width', () => {
    expect(dragRange(live, 'move', -90_000)).toEqual({ windowMs: 60_000, endOffsetMs: -90_000 })
  })

  it('stops a move at now and at the start of the span', () => {
    expect(dragRange(live, 'move', 30_000)).toEqual(live)
    expect(dragRange(live, 'move', -10 * SPAN_MS)).toEqual({ windowMs: 60_000, endOffsetMs: 60_000 - SPAN_MS })
  })

  it('widens and narrows from the left edge without moving the right one', () => {
    expect(dragRange(live, 'start', -60_000)).toEqual({ windowMs: 120_000, endOffsetMs: 0 })
    expect(dragRange(live, 'start', 59_999)).toEqual({ windowMs: MIN_WINDOW, endOffsetMs: 0 })
  })

  it('moves the right edge without moving the left one', () => {
    expect(dragRange(live, 'end', -20_000)).toEqual({ windowMs: 40_000, endOffsetMs: -20_000 })
    expect(dragRange(live, 'end', -70_000)).toEqual({ windowMs: MIN_WINDOW, endOffsetMs: -55_000 })
  })

  it('measures from where the drag started, so coming back from a limit lands on the pointer', () => {
    const pinned = dragRange(live, 'move', 50_000)
    expect(pinned).toEqual(live)
    expect(dragRange(live, 'move', -10_000)).toEqual({ windowMs: 60_000, endOffsetMs: -10_000 })
  })
})

describe('densityBuckets', () => {
  it('counts runs by when they started, and the failed ones apart', () => {
    const buckets = densityBuckets(
      [
        run({ id: 'a', claimed_at: iso(-1_000) }),
        run({ id: 'b', claimed_at: iso(-2_000), state: 'failed' }),
        run({ id: 'c', state: 'queued', claimed_at: null, completed_at: null, fire_at: iso(-299_000) }),
        run({ id: 'old', claimed_at: iso(-400_000) }),
      ],
      T,
      SPAN_MS,
      60,
    )
    expect(buckets).toHaveLength(60)
    expect(buckets[59]).toMatchObject({ count: 2, failed: 1 })
    expect(buckets[0]).toMatchObject({ count: 1, failed: 0 })
    expect(buckets.reduce((sum, b) => sum + b.count, 0)).toBe(3)
  })
})

describe('formatSpan', () => {
  it.each([
    [5_000, '5s'],
    [90_000, '1m 30s'],
    [300_000, '5m'],
  ])('%i → %s', (input, expected) => {
    expect(formatSpan(input)).toBe(expected)
  })
})
