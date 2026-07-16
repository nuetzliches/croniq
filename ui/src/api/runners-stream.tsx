import { createContext, useContext, type ReactNode } from 'react'
import { useRunnersSSE } from './hooks'
import type { RunnerSummary } from './types'

/**
 * App-wide runner SSE, lifted to a single connection.
 *
 * `useRunnersSSE` opens a `/v1/runners/stream` connection per call, so before
 * this provider the Topbar liveness dot and the Runners page would each open
 * their own. Mounting the provider once (in Layout) and having both consume
 * `useRunnersStream()` keeps it to a single stream while still driving the
 * global "live" indicator on every page.
 */

type RunnersStream = {
  data: RunnerSummary[] | undefined
  /** True while the SSE connection is open; flips false on drop + backoff. */
  isConnected: boolean
}

const RunnersStreamContext = createContext<RunnersStream | null>(null)

export function RunnersStreamProvider({ children }: { children: ReactNode }) {
  const stream = useRunnersSSE()
  return <RunnersStreamContext.Provider value={stream}>{children}</RunnersStreamContext.Provider>
}

// This provider module also exports its consumer hook. That trips
// react-refresh's "only export components" rule; the only cost is that edits
// to this small, root-level file trigger a full reload instead of HMR, which
// is fine here.
// eslint-disable-next-line react-refresh/only-export-components
export function useRunnersStream(): RunnersStream {
  const ctx = useContext(RunnersStreamContext)
  if (!ctx) {
    throw new Error('useRunnersStream must be used within a RunnersStreamProvider')
  }
  return ctx
}
