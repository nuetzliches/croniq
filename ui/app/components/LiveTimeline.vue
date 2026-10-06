<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, useTemplateRef, watch } from 'vue'
import { useElementSize, useIntervalFn, usePreferredReducedMotion } from '@vueuse/core'
import { useRouter } from 'vue-router'
import { useForecast, useJobStates, useJobs, useLiveForecast } from '~/api/queries'
import { useExecutionsStream } from '~/composables/useExecutionsStream'
import { formatAbsolute, formatDuration, formatRelative, stateLabel } from '~/lib/format'
import {
  COLLAPSED_LANES,
  DEFAULT_RANGE,
  backToNow,
  buildLanes,
  clampRange,
  densityBuckets,
  formatOffset,
  formatSpan,
  looksBack,
  type Bar,
  type Lane,
  type LaneOrder,
  type LiveRun,
  type ViewRange,
} from '~/lib/live-timeline'
import type { JobScheduleState } from '~/api/types'

/**
 * What is running right now, and what just ran — one lane per job, sliding
 * right to left past a fixed "now" line.
 *
 * The throughput chart below answers "how much, over the day"; this answers
 * "what is happening", which no other panel does at a glance. The strip right
 * of "now" carries each job's next fire, so a tick can be watched arriving.
 *
 * It also took over the "next hour" rail that used to sit beside it. Both
 * showed the future, at two horizons, in two places; now the lane labels give
 * each job's next fire and an overdue job reads as such where its runs are,
 * and the rail's histogram — the shape of the hour, "and then it gets busy" —
 * sits in the header. See "Pass 18" in docs/ui-visual-design.md.
 *
 * How it moves without re-rendering: bars are laid out once, in pixels from a
 * fixed origin `t0`, inside a layer that each animation frame merely shifts
 * left. Vue re-renders when a frame arrives from the stream (at most four
 * times a second), never per animation frame. A bar still running is drawn on
 * well past "now" and cut off by the clip, so it grows without being touched.
 *
 * What it shows is a span around "now" chosen on the range selector below
 * (issue #829): some of the past, and up to a minute of forecast. The "now"
 * line sits wherever now falls in that span.
 *
 * It can be held still: the Pause button, or moving the range's end before
 * "now", freezes the picture at one moment — runs and
 * schedule are snapshotted then, so what was on screen stays readable while
 * the stream moves on underneath. Hovering the track freezes only the motion,
 * so a tooltip can be read and a short bar clicked.
 */

const RANGE_KEY = 'croniq_live_range'
/** The width-only setting of the first range selector, read once to carry it over. */
const LEGACY_WINDOW_KEY = 'croniq_live_window'
const ORDER_KEY = 'croniq_live_order'
const EXPANDED_KEY = 'croniq_live_expanded'

/** Per-browser conveniences: a blocked storage just means the defaults. */
function readStored(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}
function writeStored(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // Remembering the choice is a convenience, not a requirement.
  }
}

/** The remembered range, or the default. A look back is never remembered. */
function readRange(): ViewRange {
  try {
    const stored = JSON.parse(readStored(RANGE_KEY) ?? 'null') as Partial<ViewRange> | null
    if (stored && Number.isFinite(stored.startMs) && Number.isFinite(stored.endMs)) {
      const range = clampRange(stored as ViewRange)
      return looksBack(range) ? backToNow(range) : range
    }
  } catch {
    // Unreadable: fall through to the older setting or the default.
  }
  const legacy = Number(readStored(LEGACY_WINDOW_KEY))
  if (Number.isFinite(legacy) && legacy > 0) return backToNow({ startMs: -legacy, endMs: 0 })
  return DEFAULT_RANGE
}

/** The visible span, as offsets from "now". */
const range = ref<ViewRange>(readRange())
watch(range, (value) => {
  if (!looksBack(value)) writeStored(RANGE_KEY, JSON.stringify(value))
})
/** How much of the past the view reaches back to. */
const pastMs = computed(() => -range.value.startMs)
/** The whole span the track is wide. */
const spanMs = computed(() => range.value.endMs - range.value.startMs)
const windowLabel = computed(() => formatSpan(spanMs.value))
const lookingBack = computed(() => looksBack(range.value))

const laneOrder = ref<LaneOrder>(readStored(ORDER_KEY) === 'next' ? 'next' : 'name')
watch(laneOrder, (value) => writeStored(ORDER_KEY, value))
const expanded = ref(readStored(EXPANDED_KEY) === '1')
watch(expanded, (value) => writeStored(EXPANDED_KEY, value ? '1' : '0'))

const { runs, connected, received, unavailable, offset } = useExecutionsStream()
const { data: jobStates, refetch: refetchJobStates } = useJobStates()
const { data: jobs } = useJobs()
const { data: forecast } = useForecast(60, 5)
const { data: liveForecast } = useLiveForecast(1)

/** The forecast as slices on the server's clock, for the range selector's future half. */
const forecastSlices = (data: typeof liveForecast.value) =>
  (data?.buckets ?? []).map((b) => ({
    start: Date.parse(b.start),
    end: Date.parse(b.end),
    count: b.count,
  }))

const serverNow = () => Date.now() + offset.value

/**
 * Held still: the moment the picture froze, with what was known then. `null`
 * while live. Runs and schedule are copied at that moment because the stream
 * keeps moving — five minutes later the runs on screen would have aged out
 * of its horizon.
 */
const frozen = shallowRef<{
  at: number
  runs: LiveRun[]
  schedule: JobScheduleState[]
  forecast: ReturnType<typeof forecastSlices>
} | null>(null)
const paused = computed(() => frozen.value !== null)

function pause() {
  if (frozen.value) return
  frozen.value = {
    at: serverNow(),
    runs: runs.value,
    schedule: jobStates.value ?? [],
    forecast: forecastSlices(liveForecast.value),
  }
}

/**
 * The range as it was before a look back began, so "Live" returns to the
 * span someone was watching rather than one shaped by how far they dragged.
 */
let lastLive: ViewRange | null = null
watch(range, (value, previous) => {
  if (looksBack(value) && !looksBack(previous)) lastLive = previous
})

/** Back to live: unfreeze, and end a look back where it started. */
function goLive() {
  frozen.value = null
  if (looksBack(range.value)) range.value = lastLive ?? backToNow(range.value)
  lastLive = null
}

/** A range ending before "now" shows the past, which only makes sense held still. */
watch(lookingBack, (back) => {
  if (back) pause()
})

/** The instant the "now" line stands for: the live clock, or the moment the view froze. */
const viewNow = () => (frozen.value ? frozen.value.at : serverNow())

/**
 * A coarse clock for what *is* re-rendered: which bars are in the window, and
 * how far the open ones are drawn. Once a second is plenty for both — the
 * smooth motion is the layer shift, not this.
 */
const coarseNow = ref(viewNow())
useIntervalFn(() => (coarseNow.value = viewNow()), 1000)
watch([runs, frozen, range], () => (coarseNow.value = viewNow()))

/**
 * Only jobs that still exist. `GET /v1/jobs/states` outlives the job on
 * purpose (#470), so a state row is no evidence of one — the rail this card
 * replaced showed deleted jobs as upcoming until it learned that. Before the
 * job list arrives, no schedule rather than a flash of the deleted ones.
 */
const schedule = computed(() => {
  if (!jobs.value) return []
  const live = new Set(jobs.value.map((job) => job.job_key))
  const states = frozen.value ? frozen.value.schedule : (jobStates.value ?? [])
  return states.filter((state) => live.has(state.job_key))
})
const sourceRuns = computed(() => (frozen.value ? frozen.value.runs : runs.value))

/**
 * A substring of the job key, case-insensitive — the same match the Runs
 * screen's search uses.
 */
const jobFilter = ref('')
const needle = computed(() => jobFilter.value.trim().toLowerCase())
const matches = (jobKey: string) => !needle.value || jobKey.toLowerCase().includes(needle.value)
const shownRuns = computed(() => sourceRuns.value.filter((r) => matches(r.job_key)))
const shownSchedule = computed(() => schedule.value.filter((s) => matches(s.job_key)))

// ─── Held lanes (next-fire order) ───────────────────────────────────────────

/**
 * In next-fire order a job's lane moves down the list the moment its next
 * fire moves on — that is, right as its run crosses "now", which is when
 * someone is watching it. So a lane whose job has just fired is held: it keeps
 * its place, greyed, for at least `HOLD_MIN_MS` and for as long as that job
 * still has a run waiting or running, up to `HOLD_MAX_MS`. Then it is released
 * and slides to its new place (the rows are positioned, not re-ordered in the
 * DOM, so the move can animate).
 */
const HOLD_MIN_MS = 5_000
const HOLD_MAX_MS = 20_000
const holds = shallowRef(new Map<string, { key: number; since: number }>())
const lastNext = new Map<string, number>()

watch(shownSchedule, (states) => {
  if (frozen.value) return
  const now = serverNow()
  let next: Map<string, { key: number; since: number }> | null = null
  for (const state of states) {
    const at = state.status === 'active' && state.next_fire_at ? Date.parse(state.next_fire_at) : NaN
    if (Number.isNaN(at)) continue
    const previous = lastNext.get(state.job_key)
    lastNext.set(state.job_key, at)
    // The next fire moved on from one that is due: the job has just fired.
    if (previous !== undefined && at > previous && previous <= now + 1_000 && !holds.value.has(state.job_key)) {
      next ??= new Map(holds.value)
      next.set(state.job_key, { key: previous, since: now })
    }
  }
  if (next) holds.value = next
})

/** Jobs with a run still waiting or running — a held lane stays while its run does. */
const openJobs = computed(
  () => new Set(shownRuns.value.filter((r) => r.completed_at === null).map((r) => r.job_key)),
)

let lastStatesRefetch = 0
useIntervalFn(() => {
  if (frozen.value) return
  const now = serverNow()
  // Release holds that have done their job.
  if (holds.value.size) {
    const kept = new Map(
      [...holds.value].filter(([job, hold]) => {
        const held = now - hold.since
        return held < HOLD_MIN_MS || (openJobs.value.has(job) && held < HOLD_MAX_MS)
      }),
    )
    if (kept.size !== holds.value.size) holds.value = kept
  }
  // The schedule is polled every 15 s; a job that has just come due would
  // keep its stale next fire that long. Ask again soon after a fire instead,
  // so the hold starts with the run rather than up to 15 s later.
  const justDue = (jobStates.value ?? []).some((s) => {
    if (s.status !== 'active' || !s.next_fire_at) return false
    const due = now - Date.parse(s.next_fire_at)
    return due >= 0 && due < 10_000
  })
  if (justDue && now - lastStatesRefetch > 2_000) {
    lastStatesRefetch = now
    void refetchJobStates()
  }
}, 1000)

const pinned = computed(() =>
  laneOrder.value === 'next' ? new Map([...holds.value].map(([job, hold]) => [job, hold.key])) : undefined,
)
const isHeld = (jobKey: string) => pinned.value?.has(jobKey) ?? false

const layout = computed(() =>
  buildLanes(
    shownRuns.value,
    shownSchedule.value,
    coarseNow.value,
    pastMs.value,
    laneOrder.value,
    pinned.value,
  ),
)

/**
 * The lanes in a stable DOM order (by job key), each with the row it is shown
 * in. Rows are positioned by `top`, so a change of order moves elements with
 * a CSS transition instead of re-inserting them, which would cut it short.
 */
const rows = computed(() =>
  layout.value.lanes
    .map((lane, row) => ({ lane, row, held: isHeld(lane.jobKey) }))
    .sort((a, b) => a.lane.jobKey.localeCompare(b.lane.jobKey)),
)

/** Past this many lanes the track scrolls, unless expanded (issue #828). */
const overflowing = computed(() => layout.value.lanes.length > COLLAPSED_LANES)

/** The strip's "now": the moment the view froze, or the live clock. */
const overviewAt = computed(() => (frozen.value ? frozen.value.at : coarseNow.value))
const overview = computed(() => densityBuckets(shownRuns.value, overviewAt.value))
const overviewForecast = computed(() =>
  frozen.value ? frozen.value.forecast : forecastSlices(liveForecast.value),
)

/** The next fire, if it falls in the part of the view right of "now". */
function nextInStrip(next: number | null) {
  // Held still, the strip right of the line is the past's future: the
  // snapshot's next fires would be stale, so it stays empty.
  if (paused.value) return false
  return next !== null && next >= coarseNow.value && next <= coarseNow.value + range.value.endMs
}

function nextLabel(lane: Lane) {
  // Looking at the past, "in 55 s" would be relative to a moment that has
  // gone; the labels stay empty until the view is back at its freeze or live.
  if (lookingBack.value) return ''
  if (lane.queuedSince !== null) {
    // Under a second the count would flicker through milliseconds; the colour
    // and the marker on the line already say it.
    const waited = coarseNow.value - lane.queuedSince
    return waited < 1_000 ? 'queued' : `queued ${formatDuration(waited)}`
  }
  if (lane.overdue) return 'overdue'
  if (lane.status && lane.status !== 'active') return lane.status
  if (lane.next === null) return ''
  return formatRelative(new Date(lane.next).toISOString(), coarseNow.value)
}

// The shape of the hour ahead. Bars, not a line: the buckets are discrete
// five-minute windows, and a line between them would imply a rate the
// scheduler does not have.
const hourBuckets = computed(() => forecast.value?.buckets ?? [])
const hourPeak = computed(() => Math.max(1, ...hourBuckets.value.map((b) => b.count)))
const hourTotal = computed(() => hourBuckets.value.reduce((sum, b) => sum + b.count, 0))

const running = computed(
  () => shownRuns.value.filter((r) => r.state === 'claimed').length,
)
const queued = computed(() => shownRuns.value.filter((r) => r.state === 'queued').length)

// ─── Links ──────────────────────────────────────────────────────────────────

/**
 * Bars and lane labels are plain anchors, not `RouterLink`s. A card with fifty
 * lanes carries hundreds of them, re-rendered with every stream frame, and a
 * component apiece was the largest single cost of a burst. One delegated
 * handler gives them in-app navigation; a modified click (new tab, new
 * window) is left to the browser, which the real `href` makes work.
 */
const router = useRouter()
function onLinkClick(event: MouseEvent) {
  if (event.defaultPrevented || event.button !== 0) return
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return
  const link = (event.target as Element).closest<HTMLAnchorElement>('a[data-route]')
  if (!link) return
  event.preventDefault()
  void router.push(link.dataset.route!)
}

// ─── Geometry ───────────────────────────────────────────────────────────────

const track = useTemplateRef<HTMLElement>('track')
const pastLayer = useTemplateRef<HTMLElement>('pastLayer')
const futureLayer = useTemplateRef<HTMLElement>('futureLayer')
const { width } = useElementSize(track)

const LANE_HEIGHT = 22
const MIN_BAR = 3

/** The origin all positions are measured from; fixed for the component's life. */
const t0 = serverNow()
/** Pixels per millisecond. */
const scale = computed(() => (spanMs.value > 0 ? width.value / spanMs.value : 0))
/** Where "now" falls on the track — past its right edge on a look back. */
const nowX = computed(() => Math.round(pastMs.value * scale.value))
/** The past is clipped at "now", or at the track's edge if now lies beyond it. */
const clipX = computed(() => Math.min(nowX.value, width.value))
const nowVisible = computed(() => nowX.value <= width.value)

/**
 * How far an open bar is drawn: past the right edge of the view, rounded up
 * to the next half minute so it changes twice a minute rather than with every
 * stream frame. The clip, not the bar's end, is what the eye sees, and a lane
 * whose only change is the clock does not have to re-render (see `v-memo`).
 */
const OPEN_STEP_MS = 30_000
const openHorizon = computed(
  () => Math.ceil((coarseNow.value + spanMs.value) / OPEN_STEP_MS) * OPEN_STEP_MS + OPEN_STEP_MS,
)

function barStyle(bar: Bar) {
  const end = bar.end ?? openHorizon.value
  const left = (bar.start - t0) * scale.value
  return {
    transform: `translateX(${left}px)`,
    width: `${Math.max(MIN_BAR, (end - bar.start) * scale.value)}px`,
  }
}

function barClass(bar: Bar) {
  // A wait still open is the news — nothing has picked the run up — and it is
  // usually a sliver at the line, so it is drawn full height and solid. Once
  // claimed it is history, and recedes to a thin line before its run.
  if (bar.kind === 'wait' && bar.end === null) return 'top-[5px] h-3 rounded-sm bg-warning'
  if (bar.kind === 'wait') return 'top-[9px] h-1 bg-warning/50'
  const tone =
    bar.state === 'claimed'
      ? 'bg-primary'
      : bar.state === 'completed'
        ? 'bg-success/70'
        : bar.state === 'failed'
          ? 'bg-error'
          : bar.state === 'dead'
            ? 'bg-error/50'
            : 'bg-accented'
  return `top-[5px] h-3 rounded-sm ${tone}`
}

// A still-open bar names when it began rather than how long it has taken:
// a duration would tie its tooltip to the clock and re-render its lane with
// every frame.
const since = (at: number) => `since ${new Date(at).toLocaleTimeString()}`

function barTitle(bar: Bar) {
  const run = bar.run
  if (bar.kind === 'wait') {
    if (bar.end === null) return `${run.job_key} — waiting for a runner ${since(bar.start)}`
    return `${run.job_key} — waited ${formatDuration(bar.end - bar.start)} before ${run.claimed_at ? 'it was claimed' : 'anything claimed it'}`
  }
  const took = bar.end === null ? since(bar.start) : formatDuration(bar.end - bar.start)
  const parts = [run.job_key, stateLabel(run.state), took]
  if (run.runner_id) parts.push(`on ${run.runner_id}`)
  if (run.attempt > 1) parts.push(`attempt ${run.attempt}`)
  return parts.join(' · ')
}

function tickStyle(at: number) {
  return { transform: `translateX(${(at - t0) * scale.value}px)` }
}

/**
 * Static labels: the time is relative, so the axis itself never moves. Five
 * evenly spaced from the left edge, minus any that would crowd the "now"
 * label, plus "now" itself where it falls.
 */
const axis = computed(() => {
  const ticks = [0, 0.2, 0.4, 0.6, 0.8]
    .map((f) => ({ x: f * width.value, label: formatOffset(range.value.startMs + f * spanMs.value) }))
    .filter((t) => !nowVisible.value || Math.abs(t.x - nowX.value) > 48)
  return nowVisible.value ? [...ticks, { x: nowX.value, label: 'now' }] : ticks
})
/** What the line stands for: "now" while live, a clock time while held still. */
const lineLabel = computed(() =>
  frozen.value ? new Date(frozen.value.at).toLocaleTimeString() : 'now',
)
const futureLabel = computed(() => formatOffset(range.value.endMs))

// ─── Motion ─────────────────────────────────────────────────────────────────

/** Pointer on the track: hold the motion so a tooltip can be read and a bar clicked. */
const hovering = ref(false)

/** The two moving layers and where each sits when no time has passed since `t0`. */
function layers(): [HTMLElement, number][] {
  const out: [HTMLElement, number][] = []
  if (pastLayer.value) out.push([pastLayer.value, nowX.value])
  if (futureLayer.value) out.push([futureLayer.value, 0])
  return out
}

/** Place the layers for the current instant, without motion. */
function shift() {
  const travelled = (viewNow() - t0) * scale.value
  for (const [layer, base] of layers()) {
    layer.style.transform = `translate3d(${base - travelled}px,0,0)`
  }
}

/**
 * The motion runs on the compositor, not the main thread. It used to be a
 * `requestAnimationFrame` loop setting a transform sixty times a second, and
 * every one of those re-ran style, pre-paint and layerization for the page.
 * That kept the main thread busy all the time, and in a burst, when the
 * stream sends a frame every quarter second and Vue has bars to patch, the
 * two competed and the card stuttered.
 *
 * A linear Web Animation per layer, over a minute, is the same motion the
 * loop drew. The compositor plays it without the main thread, which is then
 * free between stream frames. It is restarted from the current instant
 * whenever what it is drawn against changes (scale, position, a freeze, a new
 * layer), and every minute.
 */
const LEG_MS = 60_000
let animations: Animation[] = []
let legTimer: ReturnType<typeof setTimeout> | undefined

function stopAnimations() {
  for (const animation of animations) animation.cancel()
  animations = []
  clearTimeout(legTimer)
}

function animate() {
  stopAnimations()
  shift()
  if (frozen.value || hovering.value || scale.value <= 0) return
  const travelled = (viewNow() - t0) * scale.value
  const distance = LEG_MS * scale.value
  for (const [layer, base] of layers()) {
    animations.push(
      layer.animate(
        [
          { transform: `translate3d(${base - travelled}px,0,0)` },
          { transform: `translate3d(${base - travelled - distance}px,0,0)` },
        ],
        { duration: LEG_MS, easing: 'linear', fill: 'forwards' },
      ),
    )
  }
  legTimer = setTimeout(animate, LEG_MS)
}

/**
 * Smooth by default; a step a second for anyone who asked their system for
 * less motion. A five-second window crosses the card in five seconds, which
 * is exactly the kind of movement that setting exists to stop.
 */
const motion = usePreferredReducedMotion()
let stepTimer: ReturnType<typeof setInterval> | undefined

function start() {
  stopMotion()
  if (motion.value === 'reduce') {
    shift()
    stepTimer = setInterval(() => {
      if (!hovering.value) shift()
    }, 1000)
  } else {
    animate()
  }
}

function stopMotion() {
  stopAnimations()
  clearInterval(stepTimer)
}

onMounted(start)
// A new scale or position moves every bar, and a freeze or a hover stops the
// motion; restart it in the same tick so layer and bars never disagree.
watch([motion, scale, nowX, frozen, range, hovering, pastLayer, futureLayer], start, {
  flush: 'post',
})
onBeforeUnmount(stopMotion)
</script>

<template>
  <section
    class="rounded-xl border border-default bg-default p-4 shadow-sm"
    @click="onLinkClick"
  >
    <!-- Three columns, the outer two equal: the "Next hour" histogram stays
         centred however wide "Live" / "Paused" or the counts get. -->
    <div class="mb-3 flex flex-wrap items-center justify-between gap-3 sm:grid sm:grid-cols-[1fr_auto_1fr]">
      <div class="flex min-w-0 items-center gap-2">
        <span
          class="size-2 rounded-full"
          :class="paused ? 'bg-warning' : connected ? 'animate-pulse bg-success' : 'bg-accented'"
          :title="paused ? 'Paused' : connected ? 'Live' : 'Reconnecting…'"
        />
        <p
          class="cq-label"
          data-testid="live-state"
        >
          {{ paused ? 'Paused' : hovering ? 'Live · held' : 'Live' }}
        </p>
        <UButton
          v-if="paused"
          size="xs"
          variant="soft"
          icon="i-lucide-radio"
          label="Live"
          data-testid="live-resume"
          @click="goLive"
        />
        <UButton
          v-else
          size="xs"
          variant="ghost"
          color="neutral"
          icon="i-lucide-pause"
          aria-label="Pause"
          title="Hold the picture still"
          data-testid="live-pause"
          @click="pause"
        />
      </div>
      <!-- The hour ahead, from the rail this card absorbed: names are in the
           lane labels, this keeps the shape — "and then it gets busy". -->
      <div
        v-if="hourBuckets.length"
        class="hidden items-center justify-center gap-2 sm:flex"
      >
        <span class="cq-label normal-case">Next hour</span>
        <div
          class="flex h-5 w-40 items-end gap-px"
          role="img"
          :aria-label="`${hourTotal} runs due in the next hour`"
        >
          <div
            v-for="bucket in hourBuckets"
            :key="bucket.start"
            class="min-w-0 flex-1 rounded-t-[1px]"
            :class="bucket.count ? 'bg-primary/60' : 'bg-elevated'"
            :style="{ height: `${Math.max(bucket.count ? 20 : 10, (bucket.count / hourPeak) * 100)}%` }"
            :title="`${formatAbsolute(bucket.start)} — ${bucket.count} run${bucket.count === 1 ? '' : 's'}${bucket.jobs.length ? `: ${bucket.jobs.join(', ')}` : ''}`"
          />
        </div>
        <span class="cq-num text-xs text-muted">{{ hourTotal }} fire{{ hourTotal === 1 ? '' : 's' }}</span>
      </div>
      <div class="flex min-w-0 items-center justify-end gap-3 sm:col-start-3">
        <!-- Counts are of the moment shown; a past view has no such count. -->
        <span
          v-if="!lookingBack"
          class="cq-num text-xs text-muted"
        >
          {{ running }} running · {{ queued }} queued
        </span>
        <UInput
          v-model="jobFilter"
          size="xs"
          icon="i-lucide-search"
          placeholder="Filter jobs…"
          aria-label="Filter jobs"
          class="w-40"
        />
        <UButton
          size="xs"
          variant="ghost"
          color="neutral"
          :icon="laneOrder === 'name' ? 'i-lucide-arrow-down-a-z' : 'i-lucide-clock-arrow-up'"
          :label="laneOrder === 'name' ? 'Name' : 'Next fire'"
          :title="laneOrder === 'name' ? 'Lanes by job key — switch to next fire first' : 'Lanes by next fire — switch to job key'"
          data-testid="live-order"
          @click="laneOrder = laneOrder === 'name' ? 'next' : 'name'"
        />
      </div>
    </div>

    <AppEmpty
      v-if="unavailable"
      size="tight"
      icon="i-lucide-radio"
      title="Live view unavailable"
      description="This session cannot read executions, or the server predates the live stream."
    />
    <template v-else>
      <!-- Every lane is here (#828): past ten the track scrolls, lane labels
           with it, unless the card is expanded to show them all. The padding
           keeps the "now" line's cap inside the scroll box. -->
      <div
        class="mt-1 pt-2 pb-1"
        :class="!expanded && overflowing ? 'overflow-y-auto overscroll-contain' : ''"
        :style="!expanded && overflowing ? { maxHeight: `${COLLAPSED_LANES * LANE_HEIGHT + 12}px` } : {}"
        data-testid="live-lanes"
      >
        <div class="flex">
          <!-- Lane labels stay put; only the track to their right moves. -->
          <ul
            class="relative w-32 shrink-0 sm:w-56"
            :style="{ height: `${Math.max(1, layout.lanes.length) * LANE_HEIGHT}px` }"
          >
            <li
              v-for="{ lane, row, held } in rows"
              :key="lane.jobKey"
              class="absolute inset-x-0 flex h-[22px] items-center gap-2 pr-3 text-xs transition-[top,opacity] duration-500 ease-out motion-reduce:transition-none"
              :class="held ? 'opacity-40' : ''"
              :style="{ top: `${row * LANE_HEIGHT}px` }"
              :data-held="held || undefined"
            >
              <a
                :href="`/jobs/${encodeURIComponent(lane.jobKey)}`"
                :data-route="`/jobs/${encodeURIComponent(lane.jobKey)}`"
                class="min-w-0 flex-1 truncate font-mono hover:underline"
                :class="lane.overdue ? 'text-error' : lane.running ? 'text-highlighted' : 'text-primary'"
                :title="lane.jobKey"
              >
                {{ lane.jobKey }}
              </a>
              <!-- Late is not upcoming: an overdue job says so, in red, where
                   its lane shows nothing having run. -->
              <span
                v-if="lane.overdue"
                class="hidden shrink-0 items-center gap-1 text-error sm:flex"
                title="Should have fired and did not"
              >
                <UIcon
                  name="i-lucide-clock-alert"
                  class="size-3.5"
                />overdue
              </span>
              <span
                v-else
                class="cq-num hidden shrink-0 sm:inline"
                :class="lane.queuedSince !== null && !lookingBack ? 'text-warning' : 'text-muted'"
                :title="lane.next === null ? '' : formatAbsolute(new Date(lane.next).toISOString())"
              >{{ nextLabel(lane) }}</span>
            </li>
          </ul>

          <div
            ref="track"
            class="relative min-w-0 flex-1"
            :style="{ height: `${Math.max(1, layout.lanes.length) * LANE_HEIGHT}px` }"
            role="img"
            :aria-label="`${running} running and ${queued} queued; ${layout.lanes.length} jobs shown, window ${windowLabel}`"
            @pointerenter="hovering = true"
            @pointerleave="hovering = false"
          >
            <!-- Quarter gridlines, static: the axis is relative to now. -->
            <div
              v-for="tick in axis.slice(1, -1)"
              :key="tick.label"
              class="absolute inset-y-0 w-px bg-elevated"
              :style="{ left: `${tick.x}px` }"
            />

            <!-- Lane stripes, so a bar can be followed to its label. -->
            <div
              v-for="index in layout.lanes.length"
              :key="`stripe-${index}`"
              class="absolute inset-x-0 border-b border-default/50"
              :style="{ top: `${(index - 1) * LANE_HEIGHT}px`, height: `${LANE_HEIGHT}px` }"
            />

            <!-- The past: clipped at "now". -->
            <div
              class="absolute inset-y-0 left-0 overflow-hidden"
              :style="{ width: `${clipX}px` }"
            >
              <div
                ref="pastLayer"
                class="absolute inset-0 will-change-transform"
              >
                <!-- Memoised per lane: a frame re-renders the lanes it changed, a
                     new scale or position re-renders all, and a lane with an
                     open bar follows the half-minute horizon it is drawn to. -->
                <div
                  v-for="{ lane, row: index, held } in rows"
                  :key="lane.jobKey"
                  v-memo="[lane.signature, index, held, scale, lane.open ? openHorizon : 0]"
                  class="absolute inset-x-0 transition-[top,opacity] duration-500 ease-out motion-reduce:transition-none"
                  :class="held ? 'opacity-40' : ''"
                  :style="{ top: `${index * LANE_HEIGHT}px`, height: `${LANE_HEIGHT}px` }"
                >
                  <a
                    v-for="bar in lane.bars"
                    :key="bar.id"
                    :href="`/executions/${bar.run.id}`"
                    :data-route="`/executions/${bar.run.id}`"
                    class="absolute left-0 origin-left hover:brightness-125"
                    :class="barClass(bar)"
                    :style="barStyle(bar)"
                    :title="barTitle(bar)"
                  />
                </div>
              </div>
            </div>

            <!-- The near future: next fires, approaching the line. -->
            <div
              v-if="nowVisible"
              class="absolute inset-y-0 right-0 overflow-hidden"
              :style="{ left: `${nowX}px` }"
            >
              <div
                ref="futureLayer"
                class="absolute inset-0 will-change-transform"
              >
                <template
                  v-for="{ lane, row: index } in rows"
                  :key="`next-${lane.jobKey}`"
                >
                  <div
                    v-if="nextInStrip(lane.next)"
                    class="absolute left-0 w-0 border-l-2 border-dashed border-primary/60 transition-[top] duration-500 ease-out motion-reduce:transition-none"
                    :style="{
                      ...tickStyle(lane.next!),
                      top: `${index * LANE_HEIGHT + 3}px`,
                      height: `${LANE_HEIGHT - 6}px`,
                    }"
                    :title="`${lane.jobKey} — next fire`"
                  />
                </template>
              </div>
            </div>

            <!-- Now. A running lane gets a pulse where its bar meets the line. -->
            <!-- The line is the one fixed thing in a moving picture, so it is
                 drawn in the accent colour and a little past the lanes, with a
                 small cap — findable at a glance without shouting over the bars. -->
            <div
              v-if="nowVisible"
              class="pointer-events-none absolute -top-1 -bottom-1 w-0.5 rounded-full bg-primary/60"
              :style="{ left: `${nowX - 1}px` }"
            />
            <div
              v-if="nowVisible"
              class="pointer-events-none absolute -top-2 size-0 border-x-[4px] border-t-[4px] border-x-transparent border-t-primary/60"
              :style="{ left: `${nowX - 4}px` }"
            />
            <!-- Markers on the line: running (accent) and waiting for a runner
                 (yellow), so a wait of a second or two is seen while it lasts.
                 One animated container for all of them rather than an
                 animation apiece: each animated element is its own compositor
                 layer, and in a burst dozens of them made every frame re-layer
                 the card. -->
            <div
              v-if="nowVisible"
              class="pointer-events-none absolute inset-0"
              :class="paused ? '' : 'animate-pulse'"
            >
              <template
                v-for="{ lane, row: index } in rows"
                :key="`pulse-${lane.jobKey}`"
              >
                <span
                  v-if="lane.running && !paused"
                  class="absolute size-2 rounded-full bg-primary ring-2 ring-default transition-[top] duration-500 ease-out motion-reduce:transition-none"
                  :style="{ left: `${nowX - 4}px`, top: `${index * LANE_HEIGHT + 7}px` }"
                />
                <span
                  v-else-if="lane.queuedSince !== null && !lookingBack"
                  class="absolute size-2.5 rounded-full bg-warning ring-2 ring-default transition-[top] duration-500 ease-out motion-reduce:transition-none"
                  :style="{ left: `${nowX - 5}px`, top: `${index * LANE_HEIGHT + 6}px` }"
                  data-testid="live-queued-marker"
                />
              </template>
            </div>

            <p
              v-if="received && layout.lanes.length === 0"
              class="absolute inset-y-0 left-0 flex items-center text-xs text-muted"
              :style="{ width: `${clipX}px` }"
            >
              <template v-if="needle">
                No job matches “{{ jobFilter.trim() }}”.
              </template>
              <template v-else>
                No jobs defined, and nothing ran in the last {{ windowLabel }}.
              </template>
            </p>
          </div>
        </div>
      </div>

      <!-- Axis, aligned under the track. -->
      <div class="relative ml-32 mt-1 h-4 sm:ml-56">
        <span
          v-for="tick in axis"
          :key="`label-${tick.label}`"
          class="cq-label absolute -translate-x-1/2 normal-case"
          :class="tick.label === 'now' ? 'text-primary' : ''"
          :style="{ left: `${tick.x}px` }"
        >{{ tick.label === 'now' ? lineLabel : tick.label }}</span>
        <span class="cq-label absolute right-0 normal-case">{{ futureLabel }}</span>
      </div>

      <div
        v-if="overflowing"
        class="mt-1 flex items-center gap-2 text-xs text-muted"
      >
        <span>{{ layout.lanes.length }} jobs</span>
        <UButton
          size="xs"
          variant="link"
          :icon="expanded ? 'i-lucide-chevrons-down-up' : 'i-lucide-chevrons-up-down'"
          :label="expanded ? 'Collapse' : 'Expand'"
          data-testid="live-expand"
          @click="expanded = !expanded"
        />
      </div>

      <LiveRangeSelector
        v-model:range="range"
        :buckets="overview"
        :forecast="overviewForecast"
        :at="overviewAt"
      />
    </template>
  </section>
</template>
