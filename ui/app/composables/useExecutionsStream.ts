import { onScopeDispose, ref, shallowRef } from 'vue'
import { createSseStream } from '~/lib/sse'
import { ClockOffset, type LiveFrame, type LiveRun } from '~/lib/live-timeline'
import { refreshAccessToken } from '~/api/session'
import { useAuthStore } from '~/stores/auth'

/**
 * The runs of the last few minutes, live, for the dashboard timeline.
 *
 * Each frame is the whole window, so — as with the runners stream — it
 * replaces the array rather than patching it, and `shallowRef` spares Vue a
 * deep walk of rows it is about to throw away.
 *
 * `offset` is the server's clock minus this browser's. The timeline places
 * bars by server timestamps, so it reads "now" as `Date.now() + offset`.
 *
 * The dashboard opens it once and hands it to the timeline, so its counts
 * and the timeline's come from the same frame over one connection.
 */
export function useExecutionsStream() {
  const auth = useAuthStore()
  const runs = shallowRef<LiveRun[]>([])
  const connected = ref(false)
  const received = ref(false)
  /** Set when the server answered 403 or 404 — no point in reconnecting. */
  const unavailable = ref(false)
  const offset = ref(0)
  const clock = new ClockOffset()

  const stop = createSseStream({
    url: () => '/v1/executions/stream',
    getToken: () => auth.token,
    refresh: async () => (await refreshAccessToken()) !== null,
    onOpen: () => (connected.value = true),
    onClose: () => (connected.value = false),
    // 403: the session lacks `executions:read`. 404: a server older than the
    // stream. Neither answer changes on retry.
    fatalStatuses: [403, 404],
    onFatal: () => (unavailable.value = true),
    onData: (payload) => {
      const receivedAt = Date.now()
      try {
        const frame = JSON.parse(payload) as LiveFrame
        clock.add(frame.now, receivedAt)
        offset.value = clock.value
        runs.value = frame.runs
        received.value = true
      } catch {
        // A malformed frame is not worth tearing the stream down for.
      }
    },
  })

  onScopeDispose(stop)

  return { runs, connected, received, unavailable, offset }
}

export type ExecutionsStream = ReturnType<typeof useExecutionsStream>
