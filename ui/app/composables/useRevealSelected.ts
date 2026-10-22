import { nextTick, onScopeDispose, toValue, watch, type MaybeRefOrGetter, type Ref } from 'vue'

/**
 * Scroll a list to its selected row when the selection arrives from outside
 * the list: a pasted `/jobs/mail:send`, a link from the dashboard, the command
 * palette. Without it the detail pane opens beside a list still parked at the
 * top, and the row it belongs to may be a few hundred rows further down.
 *
 * The row is the element under `list` carrying `data-selected`. Each key is
 * revealed once, as soon as its row has rendered (the list may still be
 * loading when the route resolves) — after that, scrolling is the operator's.
 * A row already fully in view is left where it is, so clicking a row never
 * moves the list under the pointer.
 */
export function useRevealSelected(
  list: Ref<HTMLElement | null>,
  selected: MaybeRefOrGetter<string | null | undefined>,
  /** Anything that changes when rows render — usually the row count. */
  rendered: MaybeRefOrGetter<unknown>,
) {
  let revealed: string | null = null
  let release: (() => void) | null = null

  watch(
    () => [toValue(selected), toValue(rendered)] as const,
    async ([key]) => {
      if (!key) {
        revealed = null
        release?.()
        return
      }
      if (key === revealed) return
      await nextTick()
      const row = list.value?.querySelector<HTMLElement>('[data-selected]')
      if (!row) return
      revealed = key
      release?.()
      reveal(row)
      release = settle(row)
    },
    { immediate: true, flush: 'post' },
  )

  onScopeDispose(() => release?.())
}

function reveal(row: HTMLElement) {
  if (fullyVisible(row)) return
  const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
  // `center`, not `start`: a sticky table header would cover a row
  // scrolled to the top.
  row.scrollIntoView({ block: 'center', behavior: reduced ? 'auto' : 'smooth' })
}

/** How long a revealed row is kept in view while the rows around it settle. */
const SETTLE_MS = 1000
/** What says the operator has taken over the scrolling. */
const OPERATOR_EVENTS = ['wheel', 'touchstart', 'pointerdown', 'keydown'] as const

/**
 * Rows go on growing for a frame or two after they first render — icons and
 * cells fill in — and a scroll aimed at the last row stops at the end of the
 * list as it was, leaving that row cut off. For a moment after the reveal, and
 * only until the operator scrolls, reveal the row again whenever the rows
 * around it change size. Returns the function that stops this early.
 */
function settle(row: HTMLElement): () => void {
  const rows = row.parentElement
  if (!rows || typeof ResizeObserver === 'undefined') return () => {}
  const observer = new ResizeObserver(() => {
    if (row.isConnected) reveal(row)
  })
  observer.observe(rows)
  const scroller = document.documentElement
  const stop = () => {
    observer.disconnect()
    clearTimeout(timer)
    for (const type of OPERATOR_EVENTS) scroller.removeEventListener(type, stop, true)
  }
  const timer = setTimeout(stop, SETTLE_MS)
  for (const type of OPERATOR_EVENTS) {
    scroller.addEventListener(type, stop, { capture: true, passive: true })
  }
  return stop
}

/**
 * Inside every scrolling ancestor's box, and inside the viewport. Every one,
 * not just the nearest: a table wrapper can scroll sideways and still let the
 * page's own scroller cut the row off at the bottom.
 */
function fullyVisible(row: HTMLElement): boolean {
  const box = row.getBoundingClientRect()
  let top = 0
  let bottom = window.innerHeight
  for (let el = row.parentElement; el; el = el.parentElement) {
    const { overflowY } = getComputedStyle(el)
    if (overflowY === 'auto' || overflowY === 'scroll' || overflowY === 'hidden') {
      const frame = el.getBoundingClientRect()
      top = Math.max(top, frame.top)
      bottom = Math.min(bottom, frame.bottom)
    }
  }
  return box.top >= top && box.bottom <= bottom
}
