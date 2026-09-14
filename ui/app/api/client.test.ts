import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * What a 401 means depends on which endpoint answered it.
 *
 * Everywhere else in the API a 401 is an expired access token, and the right
 * move is to refresh and repeat the request once. On the sign-in surface it is
 * the *answer* — "that password is wrong" — and repeating it sends the wrong
 * password a second time, which the server counts a second time (issue #659).
 *
 * The React client knew this and the Vue port dropped it. These tests are here
 * so a future refactor of the retry has to decide about it on purpose.
 */

const refreshAccessToken = vi.fn()
const fetchImpl = vi.fn()

vi.mock('~/stores/auth', () => ({
  useAuthStore: () => ({ token: 'access-token' }),
}))
vi.mock('~/api/session', () => ({ refreshAccessToken }))
vi.mock('./session', () => ({ refreshAccessToken }))

/**
 * Stand in for the network. ofetch calls `globalThis.fetch`, so the count of
 * calls is the count of requests that actually went out — which is the thing
 * these tests are about.
 */
function respondWith(...outcomes: Array<{ status: number; body?: unknown }>) {
  let call = 0
  fetchImpl.mockImplementation(async () => {
    const outcome = outcomes[Math.min(call, outcomes.length - 1)]
    call += 1
    const body = JSON.stringify(outcome.body ?? {})
    return new Response(body, {
      status: outcome.status,
      headers: { 'content-type': 'application/json' },
    })
  })
}

describe('api() 401 handling', () => {
  beforeEach(async () => {
    vi.resetModules()
    refreshAccessToken.mockReset()
    fetchImpl.mockReset()
    vi.stubGlobal('fetch', fetchImpl)
  })

  it('refreshes and repeats once for an ordinary 401', async () => {
    respondWith({ status: 401 }, { status: 200, body: { ok: true } })
    refreshAccessToken.mockResolvedValue('fresh-token')
    const { api } = await import('./client')

    await expect(api('/v1/jobs')).resolves.toEqual({ ok: true })

    expect(refreshAccessToken).toHaveBeenCalledTimes(1)
    expect(fetchImpl).toHaveBeenCalledTimes(2)
  })

  it('does not refresh or replay a 401 from the sign-in surface', async () => {
    // The precondition that makes this bite: a signed-in operator on /login
    // holds a valid refresh cookie, so the refresh would succeed and the replay
    // would land — two failed attempts recorded for one mistyped password, and
    // lockout after three tries instead of five.
    respondWith({ status: 401, body: { error: 'invalid_credentials' } })
    refreshAccessToken.mockResolvedValue('fresh-token')
    const { api, ApiError } = await import('./client')

    await expect(api('/v1/auth/login')).rejects.toBeInstanceOf(ApiError)

    expect(refreshAccessToken).not.toHaveBeenCalled()
    expect(fetchImpl).toHaveBeenCalledTimes(1)
  })

  it('carries the server error body through for the sign-in surface', async () => {
    // The login form reads `body.message` to say what went wrong, so the
    // exemption must not swallow it.
    respondWith({
      status: 401,
      body: { error: 'invalid_credentials', message: 'Wrong username or password.' },
    })
    const { api, ApiError } = await import('./client')

    const caught = await api('/v1/auth/login').catch((error: unknown) => error)

    expect(caught).toBeInstanceOf(ApiError)
    expect((caught as InstanceType<typeof ApiError>).status).toBe(401)
    expect((caught as InstanceType<typeof ApiError>).message).toBe('Wrong username or password.')
    expect((caught as InstanceType<typeof ApiError>).body?.error).toBe('invalid_credentials')
  })

  it('exempts the whole /v1/auth/ prefix, not just login', async () => {
    // `/v1/auth/refresh` is the one that would recurse: refreshing in response
    // to a failed refresh is a loop with a fresh cookie read each time.
    respondWith({ status: 401 })
    const { api } = await import('./client')

    await expect(api('/v1/auth/refresh', { method: 'POST' })).rejects.toThrow()

    expect(refreshAccessToken).not.toHaveBeenCalled()
    expect(fetchImpl).toHaveBeenCalledTimes(1)
  })

  it('leaves a non-401 alone', async () => {
    respondWith({ status: 403, body: { message: 'Forbidden' } })
    const { api } = await import('./client')

    await expect(api('/v1/jobs')).rejects.toThrow('Forbidden')

    expect(refreshAccessToken).not.toHaveBeenCalled()
    expect(fetchImpl).toHaveBeenCalledTimes(1)
  })
})
