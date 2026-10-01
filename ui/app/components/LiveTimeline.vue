<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue'
import { useElementSize, useIntervalFn, usePreferredReducedMotion } from '@vueuse/core'
import { useForecast, useJobStates, useJobs } from '~/api/queries'
import { useExecutionsStream } from '~/composables/useExecutionsStream'
import { formatAbsolute, formatDuration, formatRelative, stateLabel } from '~/lib/format'
import {
  DEFAULT_WINDOW,
  NOW_AT,
  WINDOWS,
  buildLanes,
  formatOffset,
  futureSpan,
  type Bar,
  type Lane,
} from '~/lib/live-timeline'

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
 */

const STORAGE_KEY = 'croniq_live_window'

function readWindow(): number {
  try {
    const stored = Number(localStorage.getItem(STORAGE_KEY))
    if (WINDOWS.some((w) => w.value === stored)) return stored
  } catch {
    // Storage blocked: the default is fine.
  }
  return DEFAULT_WINDOW
}

const windowMs = ref<number>(readWindow())
watch(windowMs, (value) => {
  try {
    localStorage.setItem(STORAGE_KEY, String(value))
  } catch {
    // Remembering the choice is a convenience, not a requirement.
  }
})
const windowItems = WINDOWS.map((w) => ({ label: w.label, value: w.value }))
const windowLabel = computed(() => WINDOWS.find((w) => w.value === windowMs.value)?.label ?? '')

const { runs, connected, received, unavailable, offset } = useExecutionsStream()
const { data: jobStates } = useJobStates()
const { data: jobs } = useJobs()
const { data: forecast } = useForecast(60, 5)

const serverNow = () => Date.now() + offset.value

/**
 * A coarse clock for what *is* re-rendered: which bars are in the window, and
 * how far the open ones are drawn. Once a second is plenty for both — the
 * smooth motion is the layer shift, not this.
 */
const coarseNow = ref(serverNow())
useIntervalFn(() => (coarseNow.value = serverNow()), 1000)
watch(runs, () => (coarseNow.value = serverNow()))

/**
 * Only jobs that still exist. `GET /v1/jobs/states` outlives the job on
 * purpose (#470), so a state row is no evidence of one — the rail this card
 * replaced showed deleted jobs as upcoming until it learned that. Before the
 * job list arrives, no schedule rather than a flash of the deleted ones.
 */
const schedule = computed(() => {
  if (!jobs.value) return []
  const live = new Set(jobs.value.map((job) => job.job_key))
  return (jobStates.value ?? []).filter((state) => live.has(state.job_key))
})

/**
 * A substring of the job key, case-insensitive — the same match the Runs
 * screen's search uses. It narrows the lanes before they are chosen, so a
 * filtered view can show jobs the ten-lane cut would otherwise have hidden.
 */
const jobFilter = ref('')
const needle = computed(() => jobFilter.value.trim().toLowerCase())
const matches = (jobKey: string) => !needle.value || jobKey.toLowerCase().includes(needle.value)
const shownRuns = computed(() => runs.value.filter((r) => matches(r.job_key)))
const shownSchedule = computed(() => schedule.value.filter((s) => matches(s.job_key)))

const layout = computed(() =>
  buildLanes(shownRuns.value, shownSchedule.value, coarseNow.value, windowMs.value),
)

/** The next fire, if it falls in the strip right of "now". */
function nextInStrip(next: number | null) {
  return next !== null && next >= coarseNow.value && next <= coarseNow.value + futureSpan(windowMs.value)
}

function nextLabel(lane: Lane) {
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

// ─── Geometry ───────────────────────────────────────────────────────────────

const track = useTemplateRef<HTMLElement>('track')
const pastLayer = useTemplateRef<HTMLElement>('pastLayer')
const futureLayer = useTemplateRef<HTMLElement>('futureLayer')
const { width } = useElementSize(track)

const LANE_HEIGHT = 22
const MIN_BAR = 3

/** The origin all positions are measured from; fixed for the component's life. */
const t0 = serverNow()
const nowX = computed(() => Math.round(width.value * NOW_AT))
/** Pixels per millisecond. */
const scale = computed(() => (nowX.value > 0 ? nowX.value / windowMs.value : 0))

function barStyle(bar: Bar) {
  // An open bar runs on past "now" by more than a second's worth of motion,
  // so the clip — not its end — is what the eye sees until the next render.
  const end = bar.end ?? coarseNow.value + windowMs.value
  const left = (bar.start - t0) * scale.value
  return {
    transform: `translateX(${left}px)`,
    width: `${Math.max(MIN_BAR, (end - bar.start) * scale.value)}px`,
  }
}

function barClass(bar: Bar) {
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

function barTitle(bar: Bar) {
  const run = bar.run
  if (bar.kind === 'wait') {
    const waited = (bar.end ?? coarseNow.value) - bar.start
    return `${run.job_key} — waited ${formatDuration(waited)} before ${run.claimed_at ? 'it was claimed' : 'anything claimed it'}`
  }
  const took = (bar.end ?? coarseNow.value) - bar.start
  const parts = [run.job_key, stateLabel(run.state), formatDuration(took)]
  if (run.runner_id) parts.push(`on ${run.runner_id}`)
  if (run.attempt > 1) parts.push(`attempt ${run.attempt}`)
  return parts.join(' · ')
}

function tickStyle(at: number) {
  return { transform: `translateX(${(at - t0) * scale.value}px)` }
}

/** Static labels: the time is relative, so the axis itself never moves. */
const axis = computed(() => {
  const past = [-1, -0.75, -0.5, -0.25].map((f) => ({
    x: nowX.value + f * nowX.value,
    label: formatOffset(f * windowMs.value),
  }))
  return [...past, { x: nowX.value, label: 'now' }]
})
const futureLabel = computed(() => formatOffset(futureSpan(windowMs.value)))

// ─── Motion ─────────────────────────────────────────────────────────────────

function shift() {
  const travelled = (serverNow() - t0) * scale.value
  if (pastLayer.value) {
    pastLayer.value.style.transform = `translate3d(${nowX.value - travelled}px,0,0)`
  }
  if (futureLayer.value) {
    futureLayer.value.style.transform = `translate3d(${-travelled}px,0,0)`
  }
}

/**
 * Smooth by default; a step a second for anyone who asked their system for
 * less motion. A five-second window crosses the card in five seconds, which
 * is exactly the kind of movement that setting exists to stop.
 */
const motion = usePreferredReducedMotion()
let frame = 0
let stepTimer: ReturnType<typeof setInterval> | undefined

function loop() {
  shift()
  frame = requestAnimationFrame(loop)
}

function start() {
  stopMotion()
  if (motion.value === 'reduce') {
    shift()
    stepTimer = setInterval(shift, 1000)
  } else {
    frame = requestAnimationFrame(loop)
  }
}

function stopMotion() {
  cancelAnimationFrame(frame)
  clearInterval(stepTimer)
}

onMounted(start)
watch(motion, start)
// A new scale moves every bar; shift the layer in the same tick so the two
// never disagree for a frame.
watch([scale, nowX], shift, { flush: 'post' })
onBeforeUnmount(stopMotion)
</script>

<template>
  <section class="rounded-xl border border-default bg-default p-4 shadow-sm">
    <div class="mb-3 flex flex-wrap items-center justify-between gap-3">
      <div class="flex items-center gap-2">
        <span
          class="size-2 rounded-full"
          :class="connected ? 'animate-pulse bg-success' : 'bg-accented'"
          :title="connected ? 'Live' : 'Reconnecting…'"
        />
        <p class="cq-label">
          Live
        </p>
      </div>
      <!-- The hour ahead, from the rail this card absorbed: names are in the
           lane labels, this keeps the shape — "and then it gets busy". -->
      <div
        v-if="hourBuckets.length"
        class="hidden min-w-0 flex-1 items-center justify-center gap-2 sm:flex"
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
      <div class="flex items-center gap-3">
        <span class="cq-num text-xs text-muted">
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
        <USelect
          v-model="windowMs"
          :items="windowItems"
          size="xs"
          class="w-24"
          aria-label="Time window"
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
      <div class="mt-3 flex">
        <!-- Lane labels stay put; only the track to their right moves. -->
        <ul class="w-32 shrink-0 pr-3 sm:w-56">
          <li
            v-for="lane in layout.lanes"
            :key="lane.jobKey"
            class="flex h-[22px] items-center gap-2 text-xs"
          >
            <RouterLink
              :to="`/jobs/${encodeURIComponent(lane.jobKey)}`"
              class="min-w-0 flex-1 truncate font-mono hover:underline"
              :class="lane.overdue ? 'text-error' : lane.running ? 'text-highlighted' : 'text-primary'"
              :title="lane.jobKey"
            >
              {{ lane.jobKey }}
            </RouterLink>
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
              class="cq-num hidden shrink-0 text-muted sm:inline"
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
            v-for="(lane, index) in layout.lanes"
            :key="`stripe-${lane.jobKey}`"
            class="absolute inset-x-0 border-b border-default/50"
            :style="{ top: `${index * LANE_HEIGHT}px`, height: `${LANE_HEIGHT}px` }"
          />

          <!-- The past: clipped at "now". -->
          <div
            class="absolute inset-y-0 left-0 overflow-hidden"
            :style="{ width: `${nowX}px` }"
          >
            <div
              ref="pastLayer"
              class="absolute inset-0 will-change-transform"
            >
              <div
                v-for="(lane, index) in layout.lanes"
                :key="lane.jobKey"
                class="absolute inset-x-0"
                :style="{ top: `${index * LANE_HEIGHT}px`, height: `${LANE_HEIGHT}px` }"
              >
                <RouterLink
                  v-for="bar in lane.bars"
                  :key="bar.id"
                  :to="`/executions/${bar.run.id}`"
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
            class="absolute inset-y-0 right-0 overflow-hidden"
            :style="{ left: `${nowX}px` }"
          >
            <div
              ref="futureLayer"
              class="absolute inset-0 will-change-transform"
            >
              <template
                v-for="(lane, index) in layout.lanes"
                :key="`next-${lane.jobKey}`"
              >
                <div
                  v-if="nextInStrip(lane.next)"
                  class="absolute left-0 w-0 border-l-2 border-dashed border-primary/60"
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
            class="pointer-events-none absolute -top-1 -bottom-1 w-0.5 rounded-full bg-primary/60"
            :style="{ left: `${nowX - 1}px` }"
          />
          <div
            class="pointer-events-none absolute -top-2 size-0 border-x-[4px] border-t-[4px] border-x-transparent border-t-primary/60"
            :style="{ left: `${nowX - 4}px` }"
          />
          <template
            v-for="(lane, index) in layout.lanes"
            :key="`pulse-${lane.jobKey}`"
          >
            <span
              v-if="lane.running"
              class="pointer-events-none absolute size-2 animate-ping rounded-full bg-primary"
              :style="{ left: `${nowX - 4}px`, top: `${index * LANE_HEIGHT + 7}px` }"
            />
          </template>

          <p
            v-if="received && layout.lanes.length === 0"
            class="absolute inset-y-0 left-0 flex items-center text-xs text-muted"
            :style="{ width: `${nowX}px` }"
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

      <div
        v-if="layout.hidden"
        class="mt-1 text-xs text-muted"
      >
        +{{ layout.hidden }} more
      </div>

      <!-- Axis, aligned under the track. -->
      <div class="relative ml-32 mt-1 h-4 sm:ml-56">
        <span
          v-for="tick in axis"
          :key="`label-${tick.label}`"
          class="cq-label absolute -translate-x-1/2 normal-case"
          :class="tick.label === 'now' ? 'text-primary' : ''"
          :style="{ left: `${tick.x}px` }"
        >{{ tick.label }}</span>
        <span class="cq-label absolute right-0 normal-case">{{ futureLabel }}</span>
      </div>
    </template>
  </section>
</template>
