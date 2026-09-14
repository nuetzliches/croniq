import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * One entity's history, which is what the audit log is usually asked for.
 *
 * `docs/ui-screen-inventory.md` dropped the per-job Audit tab in favour of "a
 * link into the audit list, filtered by entity". Neither half was built: the
 * audit screen read three filters out of the URL and `target_id` was not among
 * them, and no screen linked to it. So "who changed this job's timeout and
 * when" was one click in the React dashboard and unreachable in this one —
 * even though `/v1/audit` has taken the parameter since it was written
 * (issue #668).
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

function lastQuery(): URLSearchParams {
  const [input] = fetchImpl.mock.calls.at(-1) as [string]
  return new URL(String(input), 'http://localhost').searchParams
}

describe('useAuditEvents', () => {
  beforeEach(() => {
    vi.resetModules()
    fetchImpl.mockReset()
    queries.length = 0
    vi.stubGlobal('fetch', fetchImpl)
    fetchImpl.mockImplementation(
      async () =>
        new Response('[]', { status: 200, headers: { 'content-type': 'application/json' } }),
    )
  })

  it('sends target_id when one entity is asked about', async () => {
    const { useAuditEvents } = await import('./queries')
    useAuditEvents(() => ({ target_type: 'job', target_id: 'etl:nightly' }))

    await queries.at(-1)!.queryFn()

    expect(lastQuery().get('target_type')).toBe('job')
    expect(lastQuery().get('target_id')).toBe('etl:nightly')
  })

  it('omits it when no entity is named', async () => {
    const { useAuditEvents } = await import('./queries')
    useAuditEvents(() => ({ actor_id: 'u-1' }))

    await queries.at(-1)!.queryFn()

    expect(lastQuery().get('target_id')).toBeNull()
    expect(lastQuery().get('actor_id')).toBe('u-1')
  })
})
