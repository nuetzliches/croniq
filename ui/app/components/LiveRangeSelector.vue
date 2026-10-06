<script setup lang="ts">
import { computed, ref, useTemplateRef } from 'vue'
import { useElementSize } from '@vueuse/core'
import {
  SPAN_MS,
  dragRange,
  formatOffset,
  formatSpan,
  type DensityBucket,
  type ViewRange,
} from '~/lib/live-timeline'

/**
 * The live timeline's window, chosen on an overview of the whole span the
 * stream covers (issue #829). It replaced a select of five fixed widths,
 * which could neither pick a width in between nor look at a moment that had
 * already passed.
 *
 * The strip is a density map of the last five minutes; the selection on it
 * is the part the track above shows. Its edges resize it, its body moves it.
 * Moving it off the right edge leaves "now" behind, which the parent treats
 * as a pause — what it shows is then a past moment, held still.
 *
 * Every drag is computed from where it started (`dragRange`), not summed per
 * pointer event, so a drag that runs into a limit and comes back lands
 * under the pointer again.
 */

const props = defineProps<{
  range: ViewRange
  buckets: DensityBucket[]
}>()

const emit = defineEmits<{
  'update:range': [range: ViewRange]
}>()

const strip = useTemplateRef<HTMLElement>('strip')
const { width } = useElementSize(strip)

const peak = computed(() => Math.max(1, ...props.buckets.map((b) => b.count)))

/** Percent of the strip from its left edge, for an offset from its right edge. */
const pct = (offsetMs: number) => ((SPAN_MS + offsetMs) / SPAN_MS) * 100
const left = computed(() => pct(props.range.endOffsetMs - props.range.windowMs))
const right = computed(() => pct(props.range.endOffsetMs))

const ticks = [-5, -4, -3, -2, -1].map((m) => ({ at: pct(m * 60_000), label: formatOffset(m * 60_000) }))

type Part = 'move' | 'start' | 'end'
let drag: { part: Part; x: number; from: ViewRange } | null = null
const dragging = ref<Part | null>(null)

function msPerPx() {
  return width.value > 0 ? SPAN_MS / width.value : 0
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
  const at = ((event.clientX - box.left) / box.width) * SPAN_MS - SPAN_MS
  const centred = dragRange(props.range, 'move', at + props.range.windowMs / 2 - props.range.endOffsetMs)
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
  const span = formatSpan(props.range.windowMs)
  return props.range.endOffsetMs === 0
    ? `${span} up to now`
    : `${span} ending ${formatOffset(props.range.endOffsetMs)} ago`.replace('−', '')
})
</script>

<template>
  <div class="mt-3 select-none">
    <div
      ref="strip"
      class="relative h-9 cursor-crosshair overflow-hidden rounded-md border border-default bg-elevated/40"
      @pointerdown.self="onStripDown"
      @pointermove="onMove"
      @pointerup="onUp"
      @pointercancel="onUp"
    >
      <!-- Activity over the span: runs started per slice, failures on top. -->
      <div class="pointer-events-none absolute inset-x-0 bottom-0 flex h-full items-end gap-px px-px pt-1">
        <div
          v-for="bucket in buckets"
          :key="bucket.start"
          class="relative min-w-0 flex-1"
          :style="{ height: `${bucket.count ? Math.max(12, (bucket.count / peak) * 100) : 0}%` }"
        >
          <div class="absolute inset-0 rounded-t-[1px] bg-primary/35" />
          <div
            v-if="bucket.failed"
            class="absolute inset-x-0 bottom-0 bg-error/70"
            :style="{ height: `${(bucket.failed / bucket.count) * 100}%` }"
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
        :style="{ width: `${100 - right}%` }"
      />

      <!-- The selection: body moves it, edges resize it. -->
      <div
        class="absolute inset-y-0 border-y-2 border-primary/70 bg-primary/5 outline-none focus-visible:ring-2 focus-visible:ring-primary"
        :class="dragging === 'move' ? 'cursor-grabbing' : 'cursor-grab'"
        :style="{ left: `${left}%`, width: `${right - left}%` }"
        tabindex="0"
        role="slider"
        aria-label="Visible time range — arrow keys move it"
        :aria-valuetext="description"
        :aria-valuenow="range.endOffsetMs"
        :aria-valuemin="range.windowMs - SPAN_MS"
        :aria-valuemax="0"
        data-testid="live-range-selection"
        @pointerdown.stop="grab('move', $event)"
        @keydown="onKey('move', $event)"
      >
        <span
          v-for="part in (['start', 'end'] as const)"
          :key="part"
          class="absolute inset-y-0 w-2.5 cursor-ew-resize rounded-sm bg-primary/80 outline-none hover:bg-primary focus-visible:ring-2 focus-visible:ring-primary"
          :class="part === 'start' ? '-left-1.5' : '-right-1.5'"
          tabindex="0"
          role="slider"
          :aria-label="part === 'start' ? 'Start of the visible range' : 'End of the visible range'"
          :aria-valuetext="description"
          :aria-valuenow="part === 'start' ? range.endOffsetMs - range.windowMs : range.endOffsetMs"
          :aria-valuemin="-SPAN_MS"
          :aria-valuemax="0"
          :data-testid="`live-range-${part}`"
          @pointerdown.stop="grab(part, $event)"
          @keydown.stop="onKey(part, $event)"
        />
      </div>
    </div>

    <div class="relative mt-1 h-4">
      <span
        v-for="tick in ticks"
        :key="tick.label"
        class="cq-label absolute normal-case"
        :class="tick.at === 0 ? '' : '-translate-x-1/2'"
        :style="{ left: `${tick.at}%` }"
      >{{ tick.label }}</span>
      <span class="cq-label absolute right-0 normal-case">now</span>
      <span class="cq-num absolute left-1/2 hidden -translate-x-1/2 text-xs text-muted sm:block">{{ description }}</span>
    </div>
  </div>
</template>
