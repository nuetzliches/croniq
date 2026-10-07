import { computed, type Ref } from 'vue'
import { useElementSize } from '@vueuse/core'
import { clampPaneWidth } from '~/lib/pane-width'
import { DETAIL_PANE_MIN_WIDTH } from '~/stores/ui'

/** The list beside a detail pane keeps at least this much. */
const LIST_MIN_WIDTH = 360
/** The handle and the `gap-1.5` either side of it: 3 × 6 px. */
const RESIZER_GUTTER = 18

/**
 * The width a resizable detail pane is shown at, beside a list.
 *
 * `stored` is the width the reader dragged to, kept per browser in the ui
 * store. The list keeps at least `LIST_MIN_WIDTH` beside it, and that bound
 * moves with the window, so it is applied here, at render, and never written
 * back — a width saved on a wide monitor survives a visit from a narrow one.
 *
 * `splitEl` is the flex row holding list, handle and pane — the caller's
 * template ref, so the template visibly owns it. `max` is the handle's upper
 * bound, `width` the pane's.
 */
export function useDetailPaneWidth(splitEl: Ref<HTMLElement | null>, stored: Ref<number>) {
  const { width: splitWidth } = useElementSize(splitEl)

  const max = computed(() =>
    // Before the first measurement the width is 0; show the saved width as is.
    Math.max(
      DETAIL_PANE_MIN_WIDTH,
      splitWidth.value > 0
        ? Math.floor(splitWidth.value - LIST_MIN_WIDTH - RESIZER_GUTTER)
        : stored.value,
    ),
  )
  const width = computed(() => clampPaneWidth(stored.value, DETAIL_PANE_MIN_WIDTH, max.value))

  return { max, width }
}
