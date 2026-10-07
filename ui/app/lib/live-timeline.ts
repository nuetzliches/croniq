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
 * How many active runs a frame carries at most: the stream's read of queued
 * and claimed runs is capped (`ACTIVE_LIMIT` in `executions_sse.rs`). A count
 * at the cap is a floor, not a figure.
 */
export const ACTIVE_CAP = 500

/** Runs running and waiting, as the store's rows have them. */
export function countActive(runs: readonly LiveRun[]): { running: number; queued: number } {
  let running = 0
  let queued = 0
  for (const run of runs) {
    if (run.state === 'claimed') running++
    else if (run.state === 'queued') queued++
  }
  return { running, queued }
}

/**
 * How far back the range selector reaches (issue #829). The stream keeps five
 * and a half minutes; the extra half minute is slack, so a bar at the left
 * edge of the widest view is never cut short by the stream's own horizon.
 */
export const SPAN_MS = 300_000

/**
 * How far past "now" the view can reach: the schedule's forecast. A fifth of
 * the past span, so "now" sits at five sixths of the range selector's strip —
 * about where the track has always drawn it — instead of being its edge.
 */
export const FUTURE_SPAN_MS = 60_000

/** The narrowest view: as narrow as a quarter-second tick still reads as live. */
export const MIN_WINDOW = 5_000

/**
 * Lanes visible before the track scrolls (issue #828). Every lane exists and
 * is reachable — by scrolling, or by expanding the card to show all of them.
 */
export const COLLAPSED_LANES = 10

/**
 * How lanes are listed: by job key, so a lane keeps its place while its job
 * fires, or by next fire, soonest first.
 */
export type LaneOrder = 'name' | 'next'


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
  /**
   * The fire time of the oldest run of this job still waiting for a runner,
   * or `null`. A wait is often a second or two — a sliver at the "now" line —
   * so the lane says so in words as well.
   */
  queuedSince: number | null
  overdue: boolean
  /** `active`, `paused`, … — `null` for a job known only from its runs. */
  status: string | null
  /**
   * What the lane's bars look like, as a string that changes exactly when
   * one of them does: which bars, in what state, ending where. The component
   * memoises each lane on it, so a stream frame re-renders only the lanes it
   * changed — in a burst a frame lands every quarter second, and patching
   * every bar of every lane each time is what made the card stutter.
   */
  signature: string
  /** Whether a bar is still open, and so grows with the clock between frames. */
  open: boolean
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
 * All of them, too (issue #828). The card used to keep ten, chosen by
 * urgency, and list them by next fire: a job that had just fired moved down
 * the list — and with enough jobs out of it — at the moment its run crossed
 * "now", which was the moment someone was watching it. The card now scrolls
 * instead of cutting, and the default order is by job key, so a lane stays
 * where it is while its job fires.
 *
 * `order: 'next'` keeps the old reading order on request: soonest next fire
 * first, the order that puts the job about to start next to the strip where
 * its tick is approaching. Jobs with no next fire (paused, manual-only, gone
 * from the schedule) follow; ties and the tail are alphabetical.
 *
 * `pinned` holds a lane at a sort key other than its next fire: the fire it
 * has just had, while its run passes "now", so it does not drop out from under
 * the eye the moment its job fires. The component decides how long.
 *
 * `favorites`, when given, puts the user's starred jobs ahead of the rest;
 * within each group the order above applies. With the card collapsed to
 * `COLLAPSED_LANES`, that is what keeps the jobs someone watches in view.
 */
export function buildLanes(
  runs: readonly LiveRun[],
  schedule: readonly JobSchedule[],
  now: number,
  windowMs: number,
  order: LaneOrder = 'name',
  pinned?: ReadonlyMap<string, number>,
  favorites?: ReadonlySet<string>,
): { lanes: Lane[] } {
  const from = now - windowMs
  const byJob = new Map<string, Lane>()
  const lane = (jobKey: string) => {
    let entry = byJob.get(jobKey)
    if (!entry) {
      entry = {
        jobKey,
        bars: [],
        next: null,
        running: false,
        queuedSince: null,
        overdue: false,
        status: null,
        signature: '',
        open: false,
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
    const fire = Date.parse(run.fire_at)
    if (run.claimed_at === null && run.completed_at === null && fire <= now) {
      const entry = lane(run.job_key)
      entry.queuedSince = entry.queuedSince === null ? fire : Math.min(entry.queuedSince, fire)
    }
    for (const bar of barsOf(run)) {
      if (bar.start > now) continue
      if (bar.end !== null && bar.end < from) continue
      const entry = lane(run.job_key)
      entry.bars.push(bar)
      entry.signature += `${bar.id}:${bar.state}:${bar.end ?? ''};`
      if (bar.end === null) entry.open = true
      if (bar.kind === 'run' && bar.end === null) entry.running = true
    }
  }

  const soonest = (l: Lane) => pinned?.get(l.jobKey) ?? (l.next === null ? Infinity : l.next)
  const byName = (a: Lane, b: Lane) => a.jobKey.localeCompare(b.jobKey)
  const within = order === 'next' ? (a: Lane, b: Lane) => soonest(a) - soonest(b) || byName(a, b) : byName
  const starred = (l: Lane) => (favorites?.has(l.jobKey) ? 0 : 1)
  const lanes = [...byJob.values()].sort((a, b) => starred(a) - starred(b) || within(a, b))
  return { lanes }
}

// ─── Range (issue #829) ─────────────────────────────────────────────────────

/**
 * What the track shows, as offsets from "now": from `startMs` (at most 0 —
 * the view always reaches back to now at least) to `endMs`, which may lie in
 * the forecast past "now" or, for a look at the past, before it. The track
 * draws its "now" line where now falls in that span.
 */
export interface ViewRange {
  startMs: number
  endMs: number
}

/**
 * One minute back and ten seconds ahead: the shape the track had before the
 * range could reach into the future, with "now" at about 85 %.
 */
export const DEFAULT_RANGE: ViewRange = { startMs: -60_000, endMs: 10_000 }

/** A view ending before "now" shows the past, which the timeline holds still. */
export function looksBack(range: ViewRange): boolean {
  return range.endMs < 0
}

const clamp = (value: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, value))

/**
 * Fit a range inside what can be shown: back to `past`, ahead to `future`,
 * starting no later than now, and at least `min` wide.
 */
export function clampRange(
  range: ViewRange,
  past = SPAN_MS,
  future = FUTURE_SPAN_MS,
  min = MIN_WINDOW,
): ViewRange {
  const endMs = clamp(Math.round(range.endMs), min - past, future)
  const startMs = clamp(Math.round(range.startMs), -past, Math.min(0, endMs - min))
  return { startMs, endMs }
}

/**
 * The same view, back at "now": its width kept, its end moved to where the
 * default puts it relative to that width. Used when leaving a look at the past.
 */
export function backToNow(range: ViewRange): ViewRange {
  const width = range.endMs - range.startMs
  const ahead = Math.round((width * DEFAULT_RANGE.endMs) / (DEFAULT_RANGE.endMs - DEFAULT_RANGE.startMs))
  return clampRange({ startMs: ahead - width, endMs: ahead })
}

/**
 * The range after dragging one part of the selection by `deltaMs`, measured
 * from where the drag started rather than accumulated per pointer event, so
 * a drag that runs into a limit and comes back lands where the pointer is.
 *
 * - `move` shifts the whole selection; its width stays.
 * - `start` moves the left edge; the right edge stays.
 * - `end` moves the right edge; the left edge stays.
 */
export function dragRange(
  from: ViewRange,
  part: 'move' | 'start' | 'end',
  deltaMs: number,
  past = SPAN_MS,
  future = FUTURE_SPAN_MS,
  min = MIN_WINDOW,
): ViewRange {
  if (part === 'move') {
    // Both edges move together, so the tightest limit on either one bounds it.
    const delta = clamp(deltaMs, -past - from.startMs, Math.min(future - from.endMs, -from.startMs))
    return { startMs: Math.round(from.startMs + delta), endMs: Math.round(from.endMs + delta) }
  }
  if (part === 'start') {
    const start = clamp(from.startMs + deltaMs, -past, Math.min(0, from.endMs - min))
    return { startMs: Math.round(start), endMs: from.endMs }
  }
  const end = clamp(from.endMs + deltaMs, from.startMs + min, future)
  return { startMs: from.startMs, endMs: Math.round(end) }
}

/** One slice of the range selector's overview: how many runs started in it. */
export interface DensityBucket {
  start: number
  end: number
  count: number
  failed: number
}

/**
 * Runs per slice of the span ending at `end`, by when they started — the
 * claim, or the fire for one nothing claimed yet. The overview the range is
 * chosen on: where the activity was, and where it failed.
 *
 * The slices sit on a fixed grid, whole multiples of their size since the
 * epoch, the grid `/v1/dashboard/forecast` cuts the future on. Cut from `end`
 * instead, every tick moved every boundary, runs a few seconds apart were
 * grouped one way and then the other, and the whole overview jumped. So there
 * is one slice more than the span holds: the first reaches back past the
 * span's start and the last is the one still filling up, cut off at `end`.
 */
export function densityBuckets(
  runs: readonly LiveRun[],
  end: number,
  span = SPAN_MS,
  count = 60,
): DensityBucket[] {
  const size = span / count
  const from = Math.floor((end - span) / size) * size
  const buckets = Array.from({ length: count + 1 }, (_, i) => ({
    start: from + i * size,
    end: from + (i + 1) * size,
    count: 0,
    failed: 0,
  }))
  for (const run of runs) {
    const at = Date.parse(run.claimed_at ?? run.fire_at)
    if (Number.isNaN(at) || at < from || at >= end) continue
    const bucket = buckets[Math.min(count, Math.floor((at - from) / size))]!
    bucket.count += 1
    if (run.state === 'failed' || run.state === 'dead') bucket.failed += 1
  }
  return buckets
}

/** `5s`, `1m 30s`, `5m` — the width of a window. */
export function formatSpan(spanMs: number): string {
  return formatOffset(-Math.abs(spanMs)).replace('−', '')
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
