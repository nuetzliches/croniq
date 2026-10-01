/**
 * Formatting for the things a run is made of: an instant, a duration, an id.
 *
 * Framework-free and tested, because these appear in every row of every list
 * and are the kind of code that looks obviously correct and is quietly wrong at
 * the boundaries — a duration of exactly 1000ms, a timestamp one second in the
 * future because two clocks disagree.
 */

/**
 * "3 min ago", "in 12 s", "just now".
 *
 * Deliberately tolerant of the future. A scheduler's rows carry fire times that
 * can be moments ahead — a queued run, or simply the server's clock a second
 * off this browser's — and `Math.abs` plus a direction reads better than "-1
 * seconds ago".
 */
export function formatRelative(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return '—'
  const then = new Date(iso).getTime()
  if (Number.isNaN(then)) return '—'

  const deltaMs = now - then
  const ahead = deltaMs < 0
  const seconds = Math.floor(Math.abs(deltaMs) / 1000)

  if (seconds < 5) return 'just now'
  const [value, unit] = scale(seconds)
  return ahead ? `in ${value} ${unit}` : `${value} ${unit} ago`
}

function scale(seconds: number): [number, string] {
  if (seconds < 60) return [seconds, 's']
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return [minutes, 'min']
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return [hours, 'h']
  const days = Math.floor(hours / 24)
  if (days < 30) return [days, 'd']
  const months = Math.floor(days / 30)
  if (months < 12) return [months, 'mo']
  return [Math.floor(months / 12), 'y']
}

/**
 * A duration, at a precision that matches its size.
 *
 * Sub-second work is reported in milliseconds because that is the difference
 * between a healthy job and a slow one; an eight-minute job does not benefit
 * from knowing it was 483,201 ms. The unit is part of the string rather than a
 * separate column so a table can right-align one thing.
 */
export function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || Number.isNaN(ms)) return '—'
  if (ms < 0) return '—'
  if (ms < 1000) return `${Math.round(ms)} ms`

  const seconds = ms / 1000
  if (seconds < 60) return `${seconds.toFixed(seconds < 10 ? 1 : 0)} s`

  const minutes = Math.floor(seconds / 60)
  const restSeconds = Math.round(seconds - minutes * 60)
  if (minutes < 60) return restSeconds ? `${minutes}m ${restSeconds}s` : `${minutes}m`

  const hours = Math.floor(minutes / 60)
  const restMinutes = minutes - hours * 60
  return restMinutes ? `${hours}h ${restMinutes}m` : `${hours}h`
}

/**
 * The first segment of a UUID — enough to recognise a run across a page,
 * short enough for a column. The full value belongs in a `title`, and callers
 * are expected to put it there.
 */
export function shortId(id: string | null | undefined): string {
  if (!id) return '—'
  return id.length > 8 ? id.slice(0, 8) : id
}

/** An absolute timestamp for the `title` behind every relative one. */
export function formatAbsolute(iso: string | null | undefined): string {
  if (!iso) return ''
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleString()
}

const clockFormatters = new Map<string, Intl.DateTimeFormat>()

/**
 * `HH:mm:ss.SSS` in the browser's zone — or in `timeZone`, which exists so a
 * test does not depend on the machine it runs on.
 *
 * The console used to slice the UTC string, so it was the one screen not in
 * the viewer's zone. The formatter is cached because the console formats up
 * to 2000 rows on every filter change. An unparseable value falls back to the
 * same slice, which is what the console showed before.
 */
export function formatClockTime(iso: string, timeZone?: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return iso.slice(11, 23)
  const key = timeZone ?? ''
  let formatter = clockFormatters.get(key)
  if (!formatter) {
    formatter = new Intl.DateTimeFormat('en-GB', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      fractionalSecondDigits: 3,
      hourCycle: 'h23',
      timeZone,
    })
    clockFormatters.set(key, formatter)
  }
  const parts: Record<string, string> = {}
  for (const p of formatter.formatToParts(date)) parts[p.type] = p.value
  return `${parts.hour}:${parts.minute}:${parts.second}.${parts.fractionalSecond}`
}

/** The IANA name of the browser's zone, for saying which zone times are in. */
export function localTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
}

/**
 * The word a state is shown as.
 *
 * `claimed` is the store's name for a run a runner holds, and a runner holds a
 * run for exactly as long as it executes it — the claim sets `started_at` in
 * the same statement, so there is no "claimed but not yet started" to tell
 * apart. "running" is what an operator is looking for, so that is the word on
 * screen; the API, the filter value and the URL keep `claimed`.
 *
 * Every other state reads as itself.
 */
export function stateLabel(state: string): string {
  return state === 'claimed' ? 'running' : state
}

/**
 * A run's log events as plain text, one line each: `timestamp LEVEL message`.
 *
 * The panel shows only the clock time, but a pasted log has left the screen
 * that said which day it was, so the full timestamp goes on the clipboard.
 */
export function formatLogLines(
  entries: ReadonlyArray<{ timestamp: string; level: string; message: string }>,
): string {
  return entries
    .map((e) => `${e.timestamp} ${e.level.toUpperCase()} ${e.message}`)
    .join('\n')
}
