<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue'
import { clampPaneWidth } from '~/lib/pane-width'

/**
 * The drag handle on the left edge of a side pane.
 *
 * It owns no width of its own: `v-model` is the pane's width in pixels, and the
 * caller decides where that is kept. The pane sits to the right of the handle,
 * so dragging left widens it.
 *
 * Also a focusable `separator`, as the ARIA pattern for a splitter asks:
 * ←/→ move it by 16 px (64 px with Shift), Home/End jump to the bounds, and a
 * double-click puts it back to the default.
 */
const props = defineProps<{
  modelValue: number
  min: number
  max: number
  defaultWidth: number
  label: string
}>()

const emit = defineEmits<{ 'update:modelValue': [width: number] }>()

let drag: { x: number; from: number } | null = null
const dragging = ref(false)

function set(width: number) {
  emit('update:modelValue', Math.round(clampPaneWidth(width, props.min, props.max)))
}

/**
 * The body's cursor and selection, for the length of a drag.
 *
 * Without it the cursor flickers back to an arrow whenever the pointer outruns
 * the 6 px strip, and the drag selects half the table's text on the way.
 */
function lockBody(locked: boolean) {
  document.body.style.cursor = locked ? 'col-resize' : ''
  document.body.style.userSelect = locked ? 'none' : ''
}

function onDown(event: PointerEvent) {
  if (event.button !== 0) return
  ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
  // From the width on screen, not the stored one: a narrow window may be
  // showing less than was saved, and the drag should start where the edge is.
  drag = { x: event.clientX, from: clampPaneWidth(props.modelValue, props.min, props.max) }
  dragging.value = true
  lockBody(true)
}

function onMove(event: PointerEvent) {
  if (!drag) return
  set(drag.from + (drag.x - event.clientX))
}

// Capture ends on its own with the pointer; only the drag state is ours.
function onUp() {
  if (!drag) return
  drag = null
  dragging.value = false
  lockBody(false)
}

// A pane closed mid-drag (Escape) must not leave the page unselectable.
onBeforeUnmount(onUp)

function onKey(event: KeyboardEvent) {
  const step = event.shiftKey ? 64 : 16
  const current = clampPaneWidth(props.modelValue, props.min, props.max)
  let next: number
  if (event.key === 'ArrowLeft') next = current + step
  else if (event.key === 'ArrowRight') next = current - step
  else if (event.key === 'Home') next = props.min
  else if (event.key === 'End') next = props.max
  else return
  // The page's own keys (the Runs list's arrows) must not act as well.
  event.preventDefault()
  event.stopPropagation()
  set(next)
}
</script>

<template>
  <div
    v-tooltip="'Drag to resize · double-click to reset'"
    role="separator"
    aria-orientation="vertical"
    :aria-label="label"
    :aria-valuenow="Math.round(clampPaneWidth(modelValue, min, max))"
    :aria-valuemin="min"
    :aria-valuemax="Math.max(min, max)"
    tabindex="0"
    :class="[
      'group flex w-1.5 shrink-0 cursor-col-resize touch-none items-center justify-center rounded-full outline-none select-none',
      'transition-colors hover:bg-primary/30 focus-visible:bg-primary/30 focus-visible:ring-2 focus-visible:ring-primary',
      dragging && 'bg-primary/40',
    ]"
    @pointerdown="onDown"
    @pointermove="onMove"
    @pointerup="onUp"
    @pointercancel="onUp"
    @dblclick="set(defaultWidth)"
    @keydown="onKey"
  >
    <span class="h-8 w-0.5 rounded-full bg-accented transition-colors group-hover:bg-primary" />
  </div>
</template>
