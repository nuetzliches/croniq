import { nextTick, toValue, watch, type MaybeRefOrGetter, type Ref } from 'vue'

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

  watch(
    () => [toValue(selected), toValue(rendered)] as const,
    async ([key]) => {
      if (!key) {
        revealed = null
        return
      }
      if (key === revealed) return
      await nextTick()
      const row = list.value?.querySelector<HTMLElement>('[data-selected]')
      if (!row) return
      revealed = key
      if (fullyVisible(row)) return
      const reduced = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
      // `center`, not `start`: a sticky table header would cover a row
      // scrolled to the top.
      row.scrollIntoView({ block: 'center', behavior: reduced ? 'auto' : 'smooth' })
    },
    { immediate: true, flush: 'post' },
  )
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
