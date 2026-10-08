import { beforeEach, describe, expect, it, vi } from 'vitest'
import { isRef, ref } from 'vue'

/**
 * What the notes inbox and its badge ask the server, and when.
 *
 * Two things are easy to get subtly wrong here. The marker has to travel as
 * the server wrote it: rebuilt from a `Date` it is cut to milliseconds, and
 * the note that set it reads as unread again. And the badge's polling has to
 * pause while the inbox is open and resume after — which only works if the
 * options are refs vue-query watches, not getters it calls once.
 */

const fetchImpl = vi.fn()
const queries: Array<Record<string, unknown>> = []
const mutations: Array<Record<string, unknown>> = []
const client = {
  invalidateQueries: vi.fn(),
  cancelQueries: vi.fn(async () => {}),
  setQueryData: vi.fn(),
}

vi.mock('~/stores/auth', () => ({
  useAuthStore: () => ({ token: 'access-token', isAuthenticated: true }),
}))
vi.mock('~/api/session', () => ({ refreshAccessToken: vi.fn() }))
vi.mock('./session', () => ({ refreshAccessToken: vi.fn() }))
vi.mock('@tanstack/vue-query', () => ({
  useQuery: (options: Record<string, unknown>) => {
    queries.push(options)
    return { data: { value: undefined } }
  },
  useMutation: (options: Record<string, unknown>) => {
    mutations.push(options)
    return options
  },
  useQueryClient: () => client,
}))

function respondWith(body: unknown) {
  fetchImpl.mockImplementation(
    async () =>
      new Response(JSON.stringify(body), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
  )
}

function lastCall(): { url: URL; method: string; body: unknown } {
  const [input, init] = fetchImpl.mock.calls.at(-1) as [string, RequestInit | undefined]
  const raw = init?.body
  return {
    url: new URL(String(input), 'http://localhost'),
    method: String(init?.method ?? 'GET').toUpperCase(),
    body: typeof raw === 'string' ? JSON.parse(raw) : raw,
  }
}

/** An option as vue-query would read it now: refs unwrapped. */
function current(value: unknown): unknown {
  return isRef(value) ? value.value : value
}

beforeEach(() => {
  vi.resetModules()
  fetchImpl.mockReset()
  queries.length = 0
  mutations.length = 0
  client.cancelQueries.mockClear()
  client.setQueryData.mockClear()
  vi.stubGlobal('fetch', fetchImpl)
  respondWith({ threads: [], latest_note_at: null, has_more: false })
})

describe('inbox requests', () => {
  it('sends the marker back verbatim, never through a Date', async () => {
    const { useNoteThreads } = await import('./queries')
    useNoteThreads(() => ({ seen_at: '2026-10-08T09:00:05.123456789Z' }))

    await (queries.at(-1)!.queryFn as () => Promise<unknown>)()

    expect(lastCall().url.pathname).toBe('/v1/notes/threads')
    expect(lastCall().url.searchParams.get('seen_at')).toBe('2026-10-08T09:00:05.123456789Z')
  })

  it('leaves the marker out on a first visit', async () => {
    const { useNoteThreads } = await import('./queries')
    useNoteThreads(() => ({ seen_at: null }))

    await (queries.at(-1)!.queryFn as () => Promise<unknown>)()

    expect(lastCall().url.searchParams.has('seen_at')).toBe(false)
  })

  it('sends mine and unread only when asked, and always a limit', async () => {
    const { noteThreadsQuery } = await import('./queries')

    expect(noteThreadsQuery({})).toEqual({ limit: '50' })
    expect(noteThreadsQuery({ mine: true, limit: 100 })).toEqual({ mine: '1', limit: '100' })
    expect(noteThreadsQuery({ unread: true, mine: false })).toEqual({ unread: '1', limit: '50' })
  })

  it('is keyed under notes, so a note written anywhere refreshes the inbox', async () => {
    const { useNoteThreads } = await import('./queries')
    useNoteThreads(() => ({}))

    const key = queries.at(-1)!.queryKey as unknown[]
    expect(key.slice(0, 2)).toEqual(['notes', 'threads'])
  })

  it('polls every 15 seconds', async () => {
    const { useNoteThreads } = await import('./queries')
    useNoteThreads(() => ({}))

    expect(current(queries.at(-1)!.refetchInterval)).toBe(15_000)
  })

  it('waits until the visit has decided what to ask', async () => {
    const decided = ref(false)
    const { useNoteThreads } = await import('./queries')
    useNoteThreads(() => ({}), decided)

    const enabled = queries.at(-1)!.enabled as { value: boolean }
    expect(enabled.value).toBe(false)
    decided.value = true
    expect(enabled.value).toBe(true)
  })
})

describe('the badge', () => {
  it('does not ask for a session with no user', async () => {
    const { useNotesSeen } = await import('./queries')
    useNotesSeen({ user: false })

    expect((queries.at(-1)!.enabled as { value: boolean }).value).toBe(false)
  })

  it('polls every 30 seconds', async () => {
    const { useNotesSeen } = await import('./queries')
    useNotesSeen({ user: true })

    expect(current(queries.at(-1)!.refetchInterval)).toBe(30_000)
    expect(current(queries.at(-1)!.refetchOnWindowFocus)).toBe(true)
  })

  it('stops while the inbox is open, and starts again after', async () => {
    const open = ref(true)
    const { useNotesSeen } = await import('./queries')
    useNotesSeen({ user: true, paused: open })
    const query = queries.at(-1)!

    // Refs, so vue-query sees the change — a getter would be read once.
    expect(isRef(query.refetchInterval)).toBe(true)
    expect(current(query.refetchInterval)).toBe(false)
    expect(current(query.refetchOnWindowFocus)).toBe(false)

    open.value = false
    expect(current(query.refetchInterval)).toBe(30_000)
    expect(current(query.refetchOnWindowFocus)).toBe(true)
  })

  it('shares its key with every other notes query', async () => {
    const { NOTES_SEEN_KEY } = await import('./queries')
    expect(NOTES_SEEN_KEY[0]).toBe('notes')
  })
})

describe('marking the inbox read', () => {
  it('hands the marker over verbatim', async () => {
    respondWith({ seen_at: '2026-10-08T09:00:05.123456789Z', unread_threads: 0 })
    const { useMarkNotesSeen } = await import('./queries')
    useMarkNotesSeen()

    await (mutations.at(-1)!.mutationFn as (v: string) => Promise<unknown>)(
      '2026-10-08T09:00:05.123456789Z',
    )

    const call = lastCall()
    expect(call.method).toBe('PUT')
    expect(call.url.pathname).toBe('/v1/users/me/notes-seen')
    expect(call.body).toEqual({ seen_at: '2026-10-08T09:00:05.123456789Z' })
  })

  it('calls off a poll in flight before writing the answer into the badge', async () => {
    const { NOTES_SEEN_KEY, useMarkNotesSeen } = await import('./queries')
    useMarkNotesSeen()
    const answer = { seen_at: '2026-10-08T10:00:00Z', unread_threads: 0 }

    await (mutations.at(-1)!.onSuccess as (seen: unknown) => Promise<void>)(answer)

    expect(client.setQueryData).toHaveBeenCalledWith(NOTES_SEEN_KEY, answer)
    // A poll that left before the PUT would otherwise land after it and put
    // the old count back.
    expect(client.cancelQueries.mock.invocationCallOrder[0]).toBeLessThan(
      client.setQueryData.mock.invocationCallOrder[0]!,
    )
  })
})
