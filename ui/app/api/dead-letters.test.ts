import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * The dead-letter mutations, at the level where the bugs were.
 *
 * Two of them shipped in the Vue rebuild and neither was visible from the
 * screen: replay sent no body at all, so the server's `force` override was
 * unreachable from the dashboard even though the 409 it answers names the flag
 * (#660); and the count came from a page of rows rather than from a count, so
 * the badge saturated and the view reported a page size as a queue size
 * (#661).
 *
 * Both are about what goes on the wire, so that is what these assert.
 */

const fetchImpl = vi.fn()

vi.mock('~/stores/auth', () => ({
  useAuthStore: () => ({ token: 'access-token', isAuthenticated: true }),
}))
vi.mock('~/api/session', () => ({ refreshAccessToken: vi.fn() }))
vi.mock('./session', () => ({ refreshAccessToken: vi.fn() }))
/** Captures the options each `useQuery` was built with, so a test can run its
 * `queryFn` and see the request that results. */
const queries: Array<{ queryFn: () => Promise<unknown> }> = []

vi.mock('@tanstack/vue-query', () => ({
  useQuery: (options: { queryFn: () => Promise<unknown> }) => {
    queries.push(options)
    return { data: { value: undefined } }
  },
  useMutation: (options: { mutationFn: (vars: unknown) => unknown }) => options,
  useQueryClient: () => ({ invalidateQueries: vi.fn() }),
}))

function okWith(body: unknown) {
  fetchImpl.mockResolvedValue(
    new Response(JSON.stringify(body), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    }),
  )
}

/** The request ofetch actually made, as URL plus parsed body. */
function lastRequest() {
  const [input, init] = fetchImpl.mock.calls.at(-1) as [string, RequestInit | undefined]
  const raw = init?.body
  return {
    url: String(input),
    method: init?.method,
    body: typeof raw === 'string' ? (JSON.parse(raw) as Record<string, unknown>) : undefined,
  }
}

/**
 * The `useMutation` stub returns the options object it was handed, so the
 * `mutationFn` is reachable — but the real hook's return type says otherwise,
 * and that is the type the compiler sees. One narrow cast, named, rather than
 * `any` scattered through the assertions.
 */
type MutationUnderTest<V> = { mutationFn: (vars: V) => Promise<unknown> }

describe('dead-letter mutations', () => {
  beforeEach(() => {
    vi.resetModules()
    fetchImpl.mockReset()
    queries.length = 0
    vi.stubGlobal('fetch', fetchImpl)
  })

  it('replays without force by default', async () => {
    okWith({ execution_id: 'e-1' })
    const { useReplayDeadLetter } = await import('./queries')

    const replay = useReplayDeadLetter() as unknown as MutationUnderTest<{
      id: string
      force?: boolean
    }>
    await replay.mutationFn({ id: 'dl-1' })

    const sent = lastRequest()
    expect(sent.url).toContain('/v1/dead-letters/dl-1/replay')
    expect(sent.method).toBe('POST')
    expect(sent.body).toEqual({ force: false })
  })

  it('can send force, which is what makes the 409 override reachable', async () => {
    okWith({ execution_id: 'e-1' })
    const { useReplayDeadLetter } = await import('./queries')

    const replay = useReplayDeadLetter() as unknown as MutationUnderTest<{
      id: string
      force?: boolean
    }>
    await replay.mutationFn({ id: 'dl-1', force: true })

    expect(lastRequest().body).toEqual({ force: true })
  })

  it('asks the count endpoint for the count, not the list for a page', async () => {
    okWith({ count: 412 })
    const { useDeadLetterCount } = await import('./queries')

    // The hook returns a computed over the query; with vue-query stubbed, the
    // request its `queryFn` makes is what there is to assert on.
    useDeadLetterCount()
    const answer = (await queries.at(-1)!.queryFn()) as { count: number }

    expect(lastRequest().url).toContain('/v1/dead-letters/count')
    expect(lastRequest().url).not.toMatch(/limit=/)
    expect(answer.count).toBe(412)
  })
})
