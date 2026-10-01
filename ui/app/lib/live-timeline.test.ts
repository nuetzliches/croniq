import { describe, expect, it } from 'vitest'
import { ClockOffset, barsOf, buildLanes, formatOffset, type LiveRun } from './live-timeline'

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

  it('chooses lanes by urgency and lists them by next fire', () => {
    const runs = [
      run({ id: '1', job_key: 'c', completed_at: iso(-1_000) }),
      run({ id: '2', job_key: 'a', completed_at: iso(-8_000), claimed_at: iso(-8_500) }),
      run({ id: '3', job_key: 'b', state: 'claimed', completed_at: null }),
    ]
    const schedule = [
      { job_key: 'z', status: 'active', next_fire_at: iso(-60_000), overdue: true },
      { job_key: 'y', status: 'active', next_fire_at: iso(5_000), overdue: false },
    ]
    const { lanes, hidden } = buildLanes(runs, schedule, T, 60_000, 3)
    // Running (b), overdue (z), most recent (c) — then listed by next fire:
    // z has one (overdue, so in the past), b and c have none and go A–Z.
    expect(lanes.map((l) => l.jobKey)).toEqual(['z', 'b', 'c'])
    expect(lanes.find((l) => l.jobKey === 'b')!.running).toBe(true)
    expect(lanes.find((l) => l.jobKey === 'z')!.overdue).toBe(true)
    expect(hidden).toBe(2)
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
  it('lists soonest next fire first, jobs without one last and A–Z', () => {
    const schedule = [
      { job_key: 'later', status: 'active', next_fire_at: iso(20 * 60_000), overdue: false },
      { job_key: 'paused', status: 'paused', next_fire_at: iso(1_000), overdue: false },
      { job_key: 'soon', status: 'active', next_fire_at: iso(30_000), overdue: false },
      { job_key: 'manual', status: 'active', next_fire_at: null, overdue: false },
    ]
    const { lanes } = buildLanes([], schedule, T, 60_000)
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
