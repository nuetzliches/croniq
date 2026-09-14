import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

/**
 * What the dashboard asks the server for when nobody is looking.
 *
 * Three places asked for more than the screen needed (issue #670). The command
 * palette is mounted in the shell for the whole session and opened rarely, and
 * its four lists were fetched on load with one of them re-polled every ten
 * seconds. Execution logs were polled every five seconds for runs that had
 * finished, where nothing more can be written. And job detail re-fetched
 * triggers the list beside it already held.
 *
 * The first two are a question about `enabled` and `refetchInterval`, which is
 * what these assert. The third is a prop, checked by the type system.
 */

const queries: Array<{
  enabled?: { value: boolean }
  refetchInterval?: unknown
}> = []

vi.mock('~/stores/auth', () => ({
  useAuthStore: () => ({ token: 'access-token', isAuthenticated: true }),
}))
vi.mock('~/api/session', () => ({ refreshAccessToken: vi.fn() }))
vi.mock('./session', () => ({ refreshAccessToken: vi.fn() }))
vi.mock('@tanstack/vue-query', () => ({
  useQuery: (options: { enabled?: { value: boolean }; refetchInterval?: unknown }) => {
    queries.push(options)
    return { data: { value: undefined } }
  },
  useMutation: (options: unknown) => options,
  useQueryClient: () => ({ invalidateQueries: vi.fn() }),
}))

/** The interval a query would use right now, resolved if it is a function. */
function intervalOf(query: { refetchInterval?: unknown }): number | false | undefined {
  const interval = query.refetchInterval
  return typeof interval === 'function'
    ? (interval as () => number | false)()
    : (interval as number | undefined)
}

describe('what gets polled', () => {
  beforeEach(() => {
    vi.resetModules()
    queries.length = 0
  })

  it('does not fetch the palette lists before it is opened', async () => {
    const opened = ref(false)
    const { useJobs, useRunners, useCalendars, useAlertsConfig } = await import('./queries')

    useJobs(opened)
    useRunners(opened)
    useCalendars(opened)
    useAlertsConfig(opened)

    expect(queries.map((q) => q.enabled?.value)).toEqual([false, false, false, false])
  })

  it('does not poll the fleet for a palette nobody opened', async () => {
    const opened = ref(false)
    const { useRunners } = await import('./queries')

    useRunners(opened)

    expect(intervalOf(queries.at(-1)!)).toBe(false)
  })

  it('polls the fleet once the palette has been opened', async () => {
    const opened = ref(true)
    const { useRunners } = await import('./queries')

    useRunners(opened)

    expect(queries.at(-1)!.enabled?.value).toBe(true)
    expect(intervalOf(queries.at(-1)!)).toBe(10_000)
  })

  it('keeps polling the logs of a run that is still going', async () => {
    const { useExecutionLogs } = await import('./queries')

    useExecutionLogs(() => 'e-1', () => 'claimed')

    expect(intervalOf(queries.at(-1)!)).toBe(5_000)
  })

  it.each(['completed', 'failed', 'dead', 'cancelled'])(
    'stops polling the logs of a %s run',
    async (state) => {
      const { useExecutionLogs } = await import('./queries')

      useExecutionLogs(() => 'e-1', () => state)

      expect(intervalOf(queries.at(-1)!)).toBe(false)
    },
  )

  it('keeps polling when the state is unknown', async () => {
    // The safe direction to be wrong in: a caller that cannot say gets the
    // old behaviour rather than a log that silently stops updating.
    const { useExecutionLogs } = await import('./queries')

    useExecutionLogs(() => 'e-1')

    expect(intervalOf(queries.at(-1)!)).toBe(5_000)
  })
})
