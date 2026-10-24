import type { LogEvent } from '~/composables/useConsoleStream'
import { formatClockTime } from './format'

/** One console line, with everything the template shows already computed. */
export interface ConsoleRow {
  event: LogEvent
  time: string
  fields: string
  gutter: string
}

export const fieldText = (event: LogEvent): string =>
  Object.entries(event.fields ?? {})
    .map(([key, value]) => `${key}=${typeof value === 'string' ? value : JSON.stringify(value)}`)
    .join(' ')

function gutterFor(level: string): string {
  if (level === 'error') return 'border-error'
  if (level === 'warn') return 'border-warning'
  return 'border-transparent'
}

/**
 * Turns console events into rows, computing each row once per event.
 *
 * The console holds up to 2 000 events and recomputed the derived text of all
 * of them — a clock-time format and a serialisation of every field map — on
 * each frame that brought one new line (review 2026-10-24, U-4). Events are
 * immutable once they arrive, so a row is cached against the event object and
 * a frame now pays for its newcomers only. The filter itself still walks the
 * buffer: a level check and a substring search per event, cheap next to what
 * the cache saves, and it has to be redone whenever the filter changes anyway.
 *
 * A `WeakMap`, so an event dropped from the buffer takes its row with it.
 */
export function createConsoleRows(format: (iso: string) => string = formatClockTime) {
  const cache = new WeakMap<LogEvent, ConsoleRow>()

  function row(event: LogEvent): ConsoleRow {
    let cached = cache.get(event)
    if (!cached) {
      cached = { event, time: format(event.ts), fields: fieldText(event), gutter: gutterFor(event.level) }
      cache.set(event, cached)
    }
    return cached
  }

  /** The rows that pass the level filter and the search, in buffer order. */
  function filter(list: readonly LogEvent[], levels: ReadonlySet<string>, search: string): ConsoleRow[] {
    const needle = search.trim().toLowerCase()
    const rows: ConsoleRow[] = []
    for (const event of list) {
      if (!levels.has(event.level)) continue
      if (
        needle &&
        !event.message.toLowerCase().includes(needle) &&
        !event.target.toLowerCase().includes(needle)
      ) {
        continue
      }
      rows.push(row(event))
    }
    return rows
  }

  return { row, filter }
}
