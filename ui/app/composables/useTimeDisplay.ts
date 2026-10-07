import { computed, onScopeDispose } from 'vue'
import { formatAbsolute, formatRelative, formatTableTime, localTimeZone } from '~/lib/format'
import { useUiStore } from '~/stores/ui'

/**
 * A table's time column, as "3 min ago" or as the clock time.
 *
 * One preference for every table (`TimeDisplay` in the ui store), so switching
 * on Runs to find the 14:00 run does not leave Dead Letters and the audit log
 * asking for arithmetic. Clock times are in the browser's zone, which the
 * column's heading names (`TimeHeading`); the tooltip carries the other form,
 * and `formatAbsolute`'s zone abbreviation for that one instant.
 *
 * Columns with words of their own — "overdue", "never" — keep them, and ask
 * this only for the instant.
 */
export function useTimeDisplay() {
  const ui = useUiStore()
  const clock = computed(() => ui.timeDisplay === 'absolute')

  function text(iso: string | null | undefined): string {
    return clock.value ? formatTableTime(iso) : formatRelative(iso)
  }

  function title(iso: string | null | undefined): string {
    const absolute = formatAbsolute(iso)
    if (!absolute) return ''
    return clock.value ? `${formatRelative(iso)} · ${absolute}` : absolute
  }

  return { clock, text, title, timeZone: localTimeZone(), toggle: ui.toggleTimeDisplay }
}

/**
 * `t` switches every time column on the screen.
 *
 * Window-wide rather than per table, because a key that worked only after
 * clicking into the right list would not be found. Every `TimeHeading` asks
 * for it, and a screen can have several — the Jobs list has two time columns,
 * and a page can hold more than one table — so the listener is shared and
 * counted: one keypress, one toggle, however many headings are mounted.
 */
let holders = 0

function onKeydown(event: KeyboardEvent) {
  if (event.key !== 't' || event.repeat) return
  // Ctrl+T and friends are the browser's.
  if (event.metaKey || event.ctrlKey || event.altKey) return
  // Never steal a key someone is typing.
  const target = event.target as HTMLElement | null
  if (target && (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))) {
    return
  }
  useUiStore().toggleTimeDisplay()
}

export function useTimeDisplayShortcut() {
  if (holders++ === 0) window.addEventListener('keydown', onKeydown)
  onScopeDispose(() => {
    if (--holders === 0) window.removeEventListener('keydown', onKeydown)
  })
}
