import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * A rolling window has to roll.
 *
 * The Runs screen asks for "the last hour", and the obvious way to build that
 * — a `computed` that subtracts an hour from `Date.now()` — produces a value
 * that never changes, because a computed tracks its reactive dependencies and
 * the clock is not one of them. The query key then holds that instant, every
 * one of the 5-second refetches re-sends it, and "the last hour" grows all day
 * (issue #662).
 *
 * So the filter carries a *length* and the request builder resolves it on each
 * fetch. These tests are about that boundary: what goes on the wire, twice,
 * with time passing in between.
 */

const fetchImpl = vi.fn()
const queries: Array<{ queryFn: () => Promise<unknown> }> = []

vi.mock('~/stores/auth', () => ({
  useAuthStore: () => ({ token: 'access-token', isAuthenticated: true }),
}))
vi.mock('~/api/session', () => ({ refreshAccessToken: vi.fn() }))
vi.mock('./session', () => ({ refreshAccessToken: vi.fn() }))
vi.mock('@tanstack/vue-query', () => ({
  useQuery: (options: { queryFn: () => Promise<unknown> }) => {
    queries.push(options)
    return { data: { value: undefined } }
  },
  useMutation: (options: unknown) => options,
  useQueryClient: () => ({ invalidateQueries: vi.fn() }),
}))

/**
 * A fresh `Response` per call.
 *
 * `mockResolvedValue` would hand back the same object every time, and a
 * response body can only be read once — the second fetch in a test would fail
 * on a consumed stream rather than on anything it meant to assert.
 */
function okWithRows() {
  fetchImpl.mockImplementation(
    async () =>
      new Response('[]', { status: 200, headers: { 'content-type': 'application/json' } }),
  )
}

/** The query string of the request that just went out. */
function lastQuery(): URLSearchParams {
  const [input] = fetchImpl.mock.calls.at(-1) as [string]
  return new URL(String(input), 'http://localhost').searchParams
}

describe('execution list requests', () => {
  beforeEach(() => {
    vi.resetModules()
    fetchImpl.mockReset()
    queries.length = 0
    vi.stubGlobal('fetch', fetchImpl)
    okWithRows()
  })

  it('resolves a window length against the clock on every fetch', async () => {
    // `Date.now` rather than fake timers: ofetch awaits real microtasks, and
    // freezing the whole timer system to move one clock reading is more
    // machinery than this needs.
    const now = vi.spyOn(Date, 'now')
    try {
      const { useExecutions } = await import('./queries')
      useExecutions(() => ({ since_ms: 3_600_000 }))
      const run = queries.at(-1)!.queryFn

      now.mockReturnValue(Date.parse('2026-09-14T12:00:00.000Z'))
      await run()
      expect(lastQuery().get('since')).toBe('2026-09-14T11:00:00.000Z')

      // The same query, half an hour later. A frozen `since` would repeat the
      // first value; a window that rolls moves with the clock.
      now.mockReturnValue(Date.parse('2026-09-14T12:30:00.000Z'))
      await run()
      expect(lastQuery().get('since')).toBe('2026-09-14T11:30:00.000Z')
    } finally {
      now.mockRestore()
    }
  })

  it('leaves an explicit since alone', async () => {
    // Paging and deep links still want a fixed lower bound.
    const { useExecutions } = await import('./queries')
    useExecutions(() => ({ since: '2026-01-01T00:00:00.000Z', since_ms: 3_600_000 }))

    await queries.at(-1)!.queryFn()

    expect(lastQuery().get('since')).toBe('2026-01-01T00:00:00.000Z')
  })

  it('sends no lower bound when no window is chosen', async () => {
    const { useExecutions } = await import('./queries')
    useExecutions(() => ({}))

    await queries.at(-1)!.queryFn()

    expect(lastQuery().get('since')).toBeNull()
  })

  it('fetchExecutions carries the full keyset cursor', async () => {
    // Paging backwards goes through this rather than through the polled query,
    // so that the polled one stays on the newest page (issue #662).
    const { fetchExecutions } = await import('./queries')

    await fetchExecutions({
      until: '2026-09-14T10:00:00.123456Z',
      until_id: 'e-42',
      since_ms: 3_600_000,
      limit: 200,
    })

    const query = lastQuery()
    expect(query.get('until')).toBe('2026-09-14T10:00:00.123456Z')
    expect(query.get('until_id')).toBe('e-42')
    expect(query.get('limit')).toBe('200')
    // The window still applies to an older page — "last hour, further back"
    // must not silently become "all time".
    expect(query.get('since')).not.toBeNull()
  })
})
