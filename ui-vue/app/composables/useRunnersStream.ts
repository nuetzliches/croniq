import { onScopeDispose, ref, shallowRef } from 'vue'
import { createSseStream } from '~/lib/sse'
import { refreshAccessToken } from '~/api/session'
import type { RunnerSummary } from '~/api/types'
import { useAuthStore } from '~/stores/auth'

/**
 * The runner fleet, live.
 *
 * This is the first consumer of `lib/sse.ts` in this tree, and the reason that
 * core was extracted before any screen needed it (#585): the transport, the
 * frame assembly across chunk boundaries, the backoff and the 401-refresh
 * dance are identical here and on the console, and writing them twice is how
 * one copy ends up subtly wrong.
 *
 * `shallowRef` for the rows: each frame replaces the whole array, so deep
 * reactivity would walk every runner's tags on every poll to discover that the
 * reference changed anyway.
 */
export function useRunnersStream() {
  const auth = useAuthStore()
  const runners = shallowRef<RunnerSummary[]>([])
  const connected = ref(false)
  /** Distinguishes "no runners" from "not connected yet" for the empty state. */
  const received = ref(false)

  const stop = createSseStream({
    url: () => '/v1/runners/stream',
    getToken: () => auth.token,
    refresh: async () => (await refreshAccessToken()) !== null,
    onOpen: () => (connected.value = true),
    onClose: () => (connected.value = false),
    onData: (payload) => {
      try {
        runners.value = JSON.parse(payload) as RunnerSummary[]
        received.value = true
      } catch {
        // A malformed frame is not worth tearing the stream down for.
      }
    },
  })

  // Scope-bound rather than tied to a component's unmount: this composable is
  // called from setup, so the scope *is* the component, and saying it this way
  // keeps it correct if it is ever called from a store or a detached scope.
  onScopeDispose(stop)

  return { runners, connected, received }
}
