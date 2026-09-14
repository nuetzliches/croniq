import { computed, onScopeDispose, ref, shallowRef } from 'vue'
import { refreshAccessToken } from '~/api/session'
import { createSseStream } from '~/lib/sse'
import { useAuthStore } from '~/stores/auth'

/**
 * The server's tracing feed, live.
 *
 * The second consumer of `lib/sse.ts`, and the one that justified extracting
 * it: the runner stream and this one differ in almost every detail that shows
 * on screen and in none of the details that are easy to get wrong — frame
 * assembly across chunk boundaries, the 401-refresh dance, reconnect backoff.
 *
 * One event as emitted by `GET /v1/events/stream`. Keep in sync with
 * `ConsoleEvent` in crates/croniq-server/src/live_console.rs.
 */
export interface LogEvent {
  ts: string
  level: 'trace' | 'debug' | 'info' | 'warn' | 'error' | string
  target: string
  message: string
  fields: Record<string, unknown>
  /**
   * A monotonic number assigned on arrival, so a row can be keyed by identity.
   *
   * The events themselves carry nothing unique — two log lines in the same
   * millisecond from the same target with the same message are indisting-
   * uishable, and rightly so. Keying the list by array index instead meant
   * every arrival shifted every key once the buffer was full, and Vue
   * re-patched all 2000 rows for one new line (issue #671).
   *
   * Not part of the wire format: assigned here, never sent.
   */
  seq: number
}

/**
 * How many events are kept.
 *
 * A live tail on a busy server would otherwise grow without bound. The cap
 * means events are *dropped*, and this composable counts them rather than
 * losing them quietly — a console that silently forgets is worse than one that
 * says it forgot.
 */
export const MAX_BUFFER = 2000

export function useConsoleStream() {
  const auth = useAuthStore()

  /** Next `seq`. Wraps at no point worth worrying about. */
  let nextSeq = 0

  const events = shallowRef<LogEvent[]>([])
  const connected = ref(false)
  /** 403: not an admin. A settled answer, not a blip — say so and stop. */
  const forbidden = ref(false)
  /** 503: the server has no console hub (an older binary, or a test build). */
  const unavailable = ref(false)
  /** How many events fell off the end of the buffer. */
  const dropped = ref(0)

  const paused = ref(false)
  /** Held while paused, flushed on resume — so pausing does not lose the tail. */
  const pending = shallowRef<LogEvent[]>([])
  const pendingCount = computed(() => pending.value.length)

  /**
   * Arrivals not yet handed to the view.
   *
   * A busy server sends events far faster than a screen can usefully show
   * them, and the previous version copied the whole 2000-element buffer per
   * event and re-assigned it — so the render cost scaled with traffic rather
   * than with the refresh rate (issue #671). Now arrivals accumulate here and
   * the buffer is rebuilt once per animation frame, which is as often as
   * anyone can see.
   */
  let batch: LogEvent[] = []
  let frame: number | null = null

  /** Drop the oldest until it fits, counting what went. */
  function trim(list: LogEvent[]): LogEvent[] {
    if (list.length <= MAX_BUFFER) return list
    dropped.value += list.length - MAX_BUFFER
    return list.slice(list.length - MAX_BUFFER)
  }

  function flush() {
    frame = null
    if (batch.length === 0) return
    const arrived = batch
    batch = []
    if (paused.value) {
      pending.value = trim([...pending.value, ...arrived])
      return
    }
    events.value = trim([...events.value, ...arrived])
  }

  function schedule() {
    if (frame !== null) return
    // `requestAnimationFrame` where there is one. In a test environment
    // without a document there is not, and a microtask is the right stand-in:
    // still a batch, just a smaller one.
    frame =
      typeof requestAnimationFrame === 'function'
        ? requestAnimationFrame(flush)
        : (queueMicrotask(flush), 1)
  }

  function push(event: LogEvent) {
    batch.push({ ...event, seq: nextSeq++ })
    schedule()
  }

  const stop = createSseStream({
    // Levels are filtered on the client, so toggling one updates the view at
    // once instead of tearing the stream down and re-opening it — hence the
    // subscription asks for everything. Backfill only on the first connect:
    // a reconnect passing `snapshot=0` does not replay what is already held.
    url: (hasOpened) => `/v1/events/stream${hasOpened ? '?snapshot=0' : ''}`,
    getToken: () => auth.token,
    refresh: async () => (await refreshAccessToken()) !== null,
    // Both are settled answers. Retrying every two seconds would only make
    // noise in a log that is itself the thing being watched.
    fatalStatuses: [403, 503],
    onFatal: (status) => {
      if (status === 403) forbidden.value = true
      if (status === 503) unavailable.value = true
    },
    // A flat two seconds rather than the exponential default: somebody is
    // watching this, and a thirty-second gap after a blip reads as broken.
    backoff: () => 2000,
    onOpen: () => (connected.value = true),
    onClose: () => (connected.value = false),
    onData: (payload) => {
      try {
        push(JSON.parse(payload) as LogEvent)
      } catch {
        // A malformed frame is not worth tearing the stream down for.
      }
    },
  })

  function resume() {
    if (pending.value.length) {
      const merged = [...events.value, ...pending.value]
      if (merged.length > MAX_BUFFER) {
        dropped.value += merged.length - MAX_BUFFER
        merged.splice(0, merged.length - MAX_BUFFER)
      }
      events.value = merged
      pending.value = []
    }
    paused.value = false
  }

  function togglePause() {
    if (paused.value) resume()
    else paused.value = true
  }

  function clear() {
    events.value = []
    pending.value = []
    dropped.value = 0
  }

  onScopeDispose(stop)

  return {
    events,
    connected,
    forbidden,
    unavailable,
    dropped,
    paused,
    pendingCount,
    togglePause,
    clear,
  }
}
