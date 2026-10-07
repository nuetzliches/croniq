<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue'
import { useElementSize, usePreferredReducedMotion } from '@vueuse/core'
import {
  FUTURE_SPAN_MS,
  SPAN_MS,
  dragRange,
  formatOffset,
  formatSpan,
  looksBack,
  type DensityBucket,
  type ViewRange,
} from '~/lib/live-timeline'

/**
 * The live timeline's span, chosen on an overview of the stream's past and
 * the schedule's near future (issue #829). It replaced a select of five fixed
 * widths, which could neither pick a width in between nor look at a moment
 * that had already passed.
 *
 * Five minutes back and one ahead, so "now" is a line at five sixths of the
 * strip — about where the track draws it — rather than its edge. Left of it,
 * runs started per slice (failures in red); right of it, the forecast: the
 * fires the schedule has coming, in outline because they are not runs yet.
 *
 * The selection is exactly what the track shows. Its left edge reaches back,
 * its right edge reaches into the forecast — or before "now", which makes the
 * view a past moment that the parent holds still — and its body moves both.
 *
 * Every drag is computed from where it started (`dragRange`), not summed per
 * pointer event, so a drag that runs into a limit and comes back lands
 * under the pointer again.
 */

const props = defineProps<{
  range: ViewRange
  /** Runs per slice of the past span, ending at `at`. */
  buckets: DensityBucket[]
  /** Fires per slice of the future, by timestamp. */
  forecast: { start: number; end: number; count: number }[]
  /** The strip's "now": the live clock, or the moment the view froze. */
  at: number
  /** Whether `at` is the live clock, so the bars glide between its ticks. */
  moving: boolean
}>()

const emit = defineEmits<{
  'update:range': [range: ViewRange]
}>()

const TOTAL = SPAN_MS + FUTURE_SPAN_MS

const strip = useTemplateRef<HTMLElement>('strip')
const { width } = useElementSize(strip)

/** Percent of the strip from its left edge, for an offset from "now". */
const pct = (offsetMs: number) => ((SPAN_MS + offsetMs) / TOTAL) * 100
const nowPct = pct(0)

const left = computed(() => pct(props.range.startMs))
const right = computed(() => pct(props.range.endMs))

/** One scale for both halves, so a busy past and a busy future compare. */
const peak = computed(() =>
  Math.max(1, ...props.buckets.map((b) => b.count), ...props.forecast.map((b) => b.count)),
)
const height = (count: number) => `${count ? Math.max(12, (count / peak.value) * 100) : 0}%`

/**
 * Where the bars are laid out from. They are placed once against this fixed
 * instant, and two layers carrying them are shifted left as time passes, the
 * way the track above moves. Placed against `at` instead, they stood still
 * between its once-a-second ticks and then jumped. Moved on now and then, so
 * the shift does not grow without bound in a tab left open for days.
 */
const origin = ref(props.at)
const REORIGIN_MS = 600_000
watch(
  () => props.at,
  (at) => {
    if (Math.abs(at - origin.value) > REORIGIN_MS) origin.value = at
  },
)

/**
 * Both halves come on one fixed grid, so the slice "now" falls in is in both:
 * its runs so far in the past, its fires still due in the forecast. Each half
 * is clipped at the line, so each shows its part of that slice.
 */
const pastBars = computed(() =>
  props.buckets.map((b) => ({
    key: b.start,
    left: pct(b.start - origin.value),
    width: ((b.end - b.start) / TOTAL) * 100,
    count: b.count,
    failed: b.failed,
  })),
)
const futureBars = computed(() =>
  props.forecast
    // What has passed is the past's to show.
    .filter((b) => b.end > props.at)
    .map((b) => ({
      key: b.start,
      left: pct(b.start - origin.value),
      width: ((b.end - b.start) / TOTAL) * 100,
      count: b.count,
    })),
)

// ─── Motion ─────────────────────────────────────────────────────────────────

/**
 * On the compositor, as in `LiveTimeline`: a linear Web Animation per layer,
 * restarted from the exact position whenever `at` ticks, so the tick only
 * corrects and never moves anything itself. Still while the view is held,
 * and a step a second for anyone who asked their system for less motion.
 */
const pastLayer = useTemplateRef<HTMLElement>('pastLayer')
const futureLayer = useTemplateRef<HTMLElement>('futureLayer')
const motion = usePreferredReducedMotion()
const LEG_MS = 60_000
let animations: Animation[] = []

function stopMotion() {
  for (const animation of animations) animation.cancel()
  animations = []
}

function place() {
  stopMotion()
  const pxPerMs = width.value / TOTAL
  const from = -(props.at - origin.value) * pxPerMs
  const glide = props.moving && motion.value !== 'reduce' && pxPerMs > 0
  for (const layer of [pastLayer.value, futureLayer.value]) {
    if (!layer) continue
    layer.style.transform = `translate3d(${from}px,0,0)`
    if (!glide) continue
    animations.push(
      layer.animate(
        [
          { transform: `translate3d(${from}px,0,0)` },
          { transform: `translate3d(${from - LEG_MS * pxPerMs}px,0,0)` },
        ],
        { duration: LEG_MS, easing: 'linear', fill: 'forwards' },
      ),
    )
  }
}

onMounted(place)
watch([() => props.at, () => props.moving, origin, width, motion, pastLayer, futureLayer], place, {
  flush: 'post',
})
onBeforeUnmount(stopMotion)

const ticks = [
  ...[-5, -4, -3, -2, -1].map((m) => ({ at: pct(m * 60_000), label: formatOffset(m * 60_000) })),
  { at: pct(30_000), label: '+30s' },
  { at: 100, label: '+1m' },
]

type Part = 'move' | 'start' | 'end'
let drag: { part: Part; x: number; from: ViewRange } | null = null
const dragging = ref<Part | null>(null)

function msPerPx() {
  return width.value > 0 ? TOTAL / width.value : 0
}

function grab(part: Part, event: PointerEvent, from = props.range) {
  ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
  drag = { part, x: event.clientX, from }
  dragging.value = part
}

function onMove(event: PointerEvent) {
  if (!drag) return
  emit('update:range', dragRange(drag.from, drag.part, (event.clientX - drag.x) * msPerPx()))
}

// Capture ends on its own with the pointer; only the drag state is ours.
function onUp() {
  if (!drag) return
  drag = null
  dragging.value = null
}

/** A press on the strip outside the selection centres the selection there and keeps dragging it. */
function onStripDown(event: PointerEvent) {
  if (!strip.value || event.button !== 0) return
  const box = strip.value.getBoundingClientRect()
  const at = ((event.clientX - box.left) / box.width) * TOTAL - SPAN_MS
  const centre = (props.range.startMs + props.range.endMs) / 2
  const centred = dragRange(props.range, 'move', at - centre)
  emit('update:range', centred)
  grab('move', event, centred)
}

/** Keyboard: arrows move (body) or resize (edges) by 5 s, 30 s with Shift. */
function onKey(part: Part, event: KeyboardEvent) {
  const step = event.shiftKey ? 30_000 : 5_000
  const delta = event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : 0
  if (!delta) return
  event.preventDefault()
  emit('update:range', dragRange(props.range, part, delta))
}

const description = computed(() => {
  const { startMs, endMs } = props.range
  if (looksBack(props.range)) {
    return `${formatSpan(endMs - startMs)} ending ${formatSpan(endMs)} ago`
  }
  const back = startMs === 0 ? 'from now' : `${formatSpan(startMs)} back`
  return endMs === 0 ? `${back}, up to now` : `${back} · ${formatSpan(endMs)} ahead`
})
</script>

<template>
  <div class="mt-3 select-none">
    <!-- Wrapper, so the "now" line can reach past the strip's clipped edges. -->
    <div class="relative">
      <div
        ref="strip"
        class="relative h-10 cursor-crosshair overflow-hidden rounded-md border border-default bg-elevated/40"
        @pointerdown.self="onStripDown"
        @pointermove="onMove"
        @pointerup="onUp"
        @pointercancel="onUp"
      >
        <!-- The future, tinted, so the line has two sides. -->
        <div
          class="pointer-events-none absolute inset-y-0 right-0 bg-primary/5"
          :style="{ left: `${nowPct}%` }"
        />

        <!-- The past: runs started per slice, failures on top. Clipped at the
             line; the moving layer is the strip's width, so the bars'
             percentages are of the strip. -->
        <div
          class="pointer-events-none absolute inset-y-0 left-0 overflow-hidden"
          :style="{ width: `${nowPct}%` }"
        >
          <div
            ref="pastLayer"
            class="absolute inset-y-0 left-0 will-change-transform"
            :style="{ width: `${width}px` }"
          >
            <div
              v-for="bar in pastBars"
              :key="`p-${bar.key}`"
              class="absolute bottom-0"
              :style="{ left: `${bar.left}%`, width: `calc(${bar.width}% - 1px)`, height: height(bar.count) }"
            >
              <div class="absolute inset-0 rounded-t-[1px] bg-primary/35" />
              <div
                v-if="bar.failed"
                class="absolute inset-x-0 bottom-0 bg-error/70"
                :style="{ height: `${(bar.failed / bar.count) * 100}%` }"
              />
            </div>
          </div>
        </div>

        <!-- The forecast: fires the schedule has coming, outlined — not yet
             runs. Clipped at the line from the other side. -->
        <div
          class="pointer-events-none absolute inset-y-0 right-0 overflow-hidden"
          :style="{ left: `${nowPct}%` }"
        >
          <div
            ref="futureLayer"
            class="absolute inset-y-0 will-change-transform"
            :style="{ left: `${(-nowPct / 100) * width}px`, width: `${width}px` }"
          >
            <div
              v-for="bar in futureBars"
              :key="`f-${bar.key}`"
              class="absolute bottom-0 rounded-t-[1px] border border-b-0 border-dashed border-primary/50 bg-primary/10"
              :style="{ left: `${bar.left}%`, width: `calc(${bar.width}% - 1px)`, height: height(bar.count) }"
              data-testid="live-range-forecast"
            />
          </div>
        </div>

        <!-- Outside the selection is dimmed, so the selection reads as the view. -->
        <div
          class="pointer-events-none absolute inset-y-0 left-0 bg-default/60"
          :style="{ width: `${left}%` }"
        />
        <div
          class="pointer-events-none absolute inset-y-0 right-0 bg-default/60"
          :style="{ left: `${right}%` }"
        />

        <!-- The selection: body moves it, edges resize it. The handles sit
             inside its edges, so at either end of the strip they are not
             clipped and are still what a press on them grabs. -->
        <div
          class="absolute inset-y-0 border-y-2 border-primary/70 bg-primary/5 outline-none focus-visible:ring-2 focus-visible:ring-primary"
          :class="dragging === 'move' ? 'cursor-grabbing' : 'cursor-grab'"
          :style="{ left: `${left}%`, width: `${right - left}%` }"
          tabindex="0"
          role="slider"
          aria-label="Visible time range — arrow keys move it"
          :aria-valuetext="description"
          :aria-valuenow="range.endMs"
          :aria-valuemin="-SPAN_MS"
          :aria-valuemax="FUTURE_SPAN_MS"
          data-testid="live-range-selection"
          @pointerdown.stop="grab('move', $event)"
          @keydown="onKey('move', $event)"
        >
          <span
            v-for="part in (['start', 'end'] as const)"
            :key="part"
            class="absolute inset-y-0 z-20 w-2.5 cursor-ew-resize rounded-sm bg-primary/80 outline-none hover:bg-primary focus-visible:ring-2 focus-visible:ring-primary"
            :class="part === 'start' ? 'left-0' : 'right-0'"
            tabindex="0"
            role="slider"
            :aria-label="part === 'start' ? 'Start of the visible range' : 'End of the visible range'"
            :aria-valuetext="description"
            :aria-valuenow="part === 'start' ? range.startMs : range.endMs"
            :aria-valuemin="-SPAN_MS"
            :aria-valuemax="part === 'start' ? 0 : FUTURE_SPAN_MS"
            :data-testid="`live-range-${part}`"
            @pointerdown.stop="grab(part, $event)"
            @keydown.stop="onKey(part, $event)"
          />
        </div>
      </div>

      <!-- Now: the one fixed line, in the strongest ink and past the strip on
           both sides, so it reads through the selection drawn over it. -->
      <div
        class="pointer-events-none absolute -top-1.5 -bottom-1.5 z-10 w-0.5 rounded-full bg-inverted ring-2 ring-default"
        :style="{ left: `calc(${nowPct}% - 1px)` }"
        data-testid="live-range-now"
      />
      <div
        class="pointer-events-none absolute -top-2.5 z-10 size-2 rounded-full bg-inverted ring-2 ring-default"
        :style="{ left: `calc(${nowPct}% - 4px)` }"
      />
    </div>

    <div class="relative mt-2 h-4">
      <span
        v-for="tick in ticks"
        :key="tick.label"
        class="cq-label absolute normal-case"
        :class="tick.at <= 0 ? '' : tick.at >= 100 ? '-translate-x-full' : '-translate-x-1/2'"
        :style="{ left: `${tick.at}%` }"
      >{{ tick.label }}</span>
      <span
        class="cq-label absolute -translate-x-1/2 font-semibold normal-case text-highlighted"
        :style="{ left: `${nowPct}%` }"
      >now</span>
    </div>
    <p class="cq-num mt-0.5 hidden text-center text-xs text-muted sm:block">
      {{ description }}
    </p>
  </div>
</template>
