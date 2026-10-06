import { describe, expect, it } from 'vitest'
import {
  ClockOffset,
  DEFAULT_RANGE,
  FUTURE_SPAN_MS,
  MIN_WINDOW,
  SPAN_MS,
  backToNow,
  barsOf,
  buildLanes,
  clampRange,
  densityBuckets,
  dragRange,
  formatOffset,
  formatSpan,
  looksBack,
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
    expect(clampRange({ startMs: -60_000, endMs: 10_000 })).toEqual({ startMs: -60_000, endMs: 10_000 })
  })

  it('reaches back to the span and ahead to the forecast, no further', () => {
    expect(clampRange({ startMs: -10 * SPAN_MS, endMs: 10 * FUTURE_SPAN_MS })).toEqual({
      startMs: -SPAN_MS,
      endMs: FUTURE_SPAN_MS,
    })
  })

  it('always starts at now or before, and is at least the minimum wide', () => {
    expect(clampRange({ startMs: 20_000, endMs: 40_000 }).startMs).toBe(0)
    // Too narrow: the end stays, the start gives way.
    expect(clampRange({ startMs: -10_000, endMs: -9_000 })).toEqual({ startMs: -9_000 - MIN_WINDOW, endMs: -9_000 })
  })
})

describe('dragRange', () => {
  const live = { startMs: -60_000, endMs: 10_000 }

  it('pulls the right edge past now into the forecast, up to its end (#829)', () => {
    expect(dragRange(live, 'end', 30_000)).toEqual({ startMs: -60_000, endMs: 40_000 })
    expect(dragRange(live, 'end', 10 * FUTURE_SPAN_MS)).toEqual({ startMs: -60_000, endMs: FUTURE_SPAN_MS })
  })

  it('pulls the right edge before now, which is a look back', () => {
    const back = dragRange(live, 'end', -30_000)
    expect(back).toEqual({ startMs: -60_000, endMs: -20_000 })
    expect(looksBack(back)).toBe(true)
    expect(looksBack(live)).toBe(false)
  })

  it('keeps the minimum width against either edge', () => {
    expect(dragRange(live, 'end', -10 * SPAN_MS)).toEqual({ startMs: -60_000, endMs: -60_000 + MIN_WINDOW })
    expect(dragRange(live, 'start', 10 * SPAN_MS)).toEqual({ startMs: 0, endMs: 10_000 })
  })

  it('moves the whole selection, keeping its width, within both limits', () => {
    expect(dragRange(live, 'move', -90_000)).toEqual({ startMs: -150_000, endMs: -80_000 })
    // The forecast's end stops it on the right …
    expect(dragRange(live, 'move', 10 * SPAN_MS)).toEqual({ startMs: -10_000, endMs: FUTURE_SPAN_MS })
    // … or now, if the start would otherwise pass it first.
    expect(dragRange({ startMs: -20_000, endMs: 30_000 }, 'move', 60_000)).toEqual({ startMs: 0, endMs: 50_000 })
    expect(dragRange(live, 'move', -10 * SPAN_MS)).toEqual({ startMs: -SPAN_MS, endMs: -230_000 })
  })

  it('measures from where the drag started, so coming back from a limit lands on the pointer', () => {
    expect(dragRange(live, 'move', 500_000)).toEqual(dragRange(live, 'move', 60_000))
    expect(dragRange(live, 'move', -10_000)).toEqual({ startMs: -70_000, endMs: 0 })
  })
})

describe('backToNow', () => {
  it('keeps the width and puts the end where the default share puts it', () => {
    const back = { startMs: -200_000, endMs: -130_000 }
    expect(backToNow(back)).toEqual(DEFAULT_RANGE)
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

describe('queued lanes', () => {
  it('remembers the oldest fire still waiting for a runner', () => {
    const waiting = { state: 'queued', claimed_at: null, completed_at: null }
    const { lanes } = buildLanes(
      [
        run({ id: 'w1', job_key: 'mail:receive', ...waiting, fire_at: iso(-3_000) }),
        run({ id: 'w2', job_key: 'mail:receive', ...waiting, fire_at: iso(-1_000) }),
        run({ id: 'done', job_key: 'sms:receive' }),
      ],
      [],
      T,
      60_000,
    )
    expect(lanes.find((l) => l.jobKey === 'mail:receive')!.queuedSince).toBe(T - 3_000)
    expect(lanes.find((l) => l.jobKey === 'sms:receive')!.queuedSince).toBeNull()
  })
})
