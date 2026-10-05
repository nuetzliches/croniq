/**
 * The arithmetic behind the dashboard's live timeline, kept out of the
 * component so it can be tested without a DOM or a clock.
 *
 * Times are milliseconds on the *server's* clock throughout. The bars come
 * from server timestamps, and at the narrowest window a one-second skew
 * between server and browser would misplace every one of them by a fifth of
 * the width — so the component converts its own clock once, through
 * {@link ClockOffset}, and nothing here ever reads `Date.now()`.
 */

/** One run, as `GET /v1/executions/stream` sends it. */
export interface LiveRun {
  id: string
  job_key: string
  state: string
  attempt: number
  runner_id: string | null
  fire_at: string
  claimed_at: string | null
  completed_at: string | null
}

export interface LiveFrame {
  /** The server's clock when the frame was read. */
  now: string
  runs: LiveRun[]
}

/**
 * The windows on offer. Five minutes is the widest the stream covers (its
 * horizon is five and a half); five seconds is as narrow as a quarter-second
 * tick still reads as live.
 */
export const WINDOWS = [
  { label: '5 s', value: 5_000 },
  { label: '10 s', value: 10_000 },
  { label: '30 s', value: 30_000 },
  { label: '1 min', value: 60_000 },
  { label: '5 min', value: 300_000 },
] as const

export const DEFAULT_WINDOW = 60_000

/**
 * Where the "now" line sits, as a fraction of the track. Left of it is the
 * window; right of it a strip of the near future, where the next fires wait.
 */
export const NOW_AT = 0.85

/** Lanes shown before the rest collapse into "+N more". */
export const MAX_LANES = 10

/** How far into the future the strip right of "now" reaches. */
export function futureSpan(windowMs: number): number {
  return (windowMs * (1 - NOW_AT)) / NOW_AT
}

/**
 * Server clock minus browser clock.
 *
 * Each sample is `server.now - receivedAt`, which undershoots the true offset
 * by however long the frame was in flight. The largest recent sample is the
 * one that was in flight the shortest, so that is the estimate — the same
 * reasoning NTP's minimum-delay filter uses.
 */
export class ClockOffset {
  private samples: number[] = []

  constructor(private readonly keep = 20) {}

  add(serverIso: string, receivedAt: number): void {
    const server = Date.parse(serverIso)
    if (Number.isNaN(server)) return
    this.samples.push(server - receivedAt)
    if (this.samples.length > this.keep) this.samples.shift()
  }

  get value(): number {
    return this.samples.length ? Math.max(...this.samples) : 0
  }
}

/**
 * One horizontal segment. `end: null` is still open — a run in progress, or
 * a fire still waiting — and is drawn on past the "now" line, where the clip
 * cuts it off; that way it grows without being re-rendered every frame.
 */
export interface Bar {
  id: string
  kind: 'wait' | 'run'
  state: string
  start: number
  end: number | null
  run: LiveRun
}

const ms = (iso: string | null) => (iso === null ? null : Date.parse(iso))

/**
 * A run's bars: the wait from its fire to its claim, then the execution.
 *
 * The wait is what makes a blocked or queued fire visible at all — without
 * it, a job that waited four minutes for a slot looks the same as one that
 * started on time. A run cancelled or killed before anything claimed it has
 * only the wait, ending where it was given up.
 */
export function barsOf(run: LiveRun): Bar[] {
  const fire = ms(run.fire_at)!
  const claimed = ms(run.claimed_at)
  const completed = ms(run.completed_at)
  const bars: Bar[] = []

  const waitEnd = claimed ?? completed
  if (waitEnd === null || waitEnd > fire) {
    bars.push({ id: `${run.id}:wait`, kind: 'wait', state: run.state, start: fire, end: waitEnd, run })
  }
  if (claimed !== null) {
    bars.push({ id: `${run.id}:run`, kind: 'run', state: run.state, start: claimed, end: completed, run })
  }
  return bars
}

export interface Lane {
  jobKey: string
  bars: Bar[]
  /** The job's next scheduled fire, wherever it falls. */
  next: number | null
  running: boolean
  overdue: boolean
  /** `active`, `paused`, … — `null` for a job known only from its runs. */
  status: string | null
}

/** What the lane labels need from `GET /v1/jobs/states`. */
export interface JobSchedule {
  job_key: string
  status: string
  next_fire_at: string | null
  overdue: boolean
}

/**
 * One lane per job: every scheduled job, plus any job that ran in the window
 * without being in the schedule (a manual trigger of a job since removed).
 *
 * Every job rather than only the busy ones, because this card replaced the
 * "next hour" rail: its labels carry each job's next fire, and a job that is
 * quiet right now is exactly the one whose "in 22 min" someone came to read.
 *
 * Lanes are listed by next fire, soonest first — the order the "next hour"
 * rail read in, and the one that puts the job about to start next to the
 * strip where its tick is approaching. Jobs with no next fire (paused,
 * manual-only, gone from the schedule) follow, alphabetically. The cost is
 * that a lane moves down the list once its job fires; ties and the tail are
 * alphabetical so nothing else shuffles.
 *
 * Urgency decides *which* jobs get a lane once there are more than
 * `maxLanes`: running, then overdue, then whatever ran most recently, then
 * whatever fires soonest.
 */
export function buildLanes(
  runs: readonly LiveRun[],
  schedule: readonly JobSchedule[],
  now: number,
  windowMs: number,
  maxLanes = MAX_LANES,
): { lanes: Lane[]; hidden: number } {
  const from = now - windowMs
  const byJob = new Map<string, Lane & { last: number }>()
  const lane = (jobKey: string) => {
    let entry = byJob.get(jobKey)
    if (!entry) {
      entry = {
        jobKey,
        bars: [],
        next: null,
        running: false,
        overdue: false,
        status: null,
        last: -Infinity,
      }
      byJob.set(jobKey, entry)
    }
    return entry
  }

  for (const job of schedule) {
    const entry = lane(job.job_key)
    entry.status = job.status
    entry.overdue = job.overdue
    const next = job.next_fire_at === null ? NaN : Date.parse(job.next_fire_at)
    // Only an active job's next fire is a promise; a paused one keeps a
    // timestamp nothing will honour.
    entry.next = job.status === 'active' && !Number.isNaN(next) ? next : null
  }
  for (const run of runs) {
    for (const bar of barsOf(run)) {
      if (bar.start > now) continue
      if (bar.end !== null && bar.end < from) continue
      const entry = lane(run.job_key)
      entry.bars.push(bar)
      entry.last = Math.max(entry.last, bar.end ?? now)
      if (bar.kind === 'run' && bar.end === null) entry.running = true
    }
  }

  const soonest = (l: Lane) => (l.next === null ? Infinity : l.next)
  const ranked = [...byJob.values()].sort(
    (a, b) =>
      Number(b.running) - Number(a.running) ||
      Number(b.overdue) - Number(a.overdue) ||
      b.last - a.last ||
      soonest(a) - soonest(b),
  )
  const shown = ranked
    .slice(0, maxLanes)
    .sort((a, b) => soonest(a) - soonest(b) || a.jobKey.localeCompare(b.jobKey))
    .map(({ jobKey, bars, next, running, overdue, status }) => ({
      jobKey,
      bars,
      next,
      running,
      overdue,
      status,
    }))
  return { lanes: shown, hidden: Math.max(0, ranked.length - maxLanes) }
}

/** `-30s`, `-1m`, `-2m 30s`, `+5s` — an axis label relative to now. */
export function formatOffset(offsetMs: number): string {
  const sign = offsetMs < 0 ? '−' : offsetMs > 0 ? '+' : ''
  const total = Math.round(Math.abs(offsetMs) / 1000)
  if (total === 0) return 'now'
  const minutes = Math.floor(total / 60)
  const seconds = total % 60
  if (!minutes) return `${sign}${seconds}s`
  return seconds ? `${sign}${minutes}m ${seconds}s` : `${sign}${minutes}m`
}
