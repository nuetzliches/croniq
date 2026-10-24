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

const refreshSession = vi.fn()
const fetchImpl = vi.fn()

const store: { token: string | null } = { token: 'access-token' }
vi.mock('~/stores/auth', () => ({
  useAuthStore: () => store,
}))
vi.mock('~/api/session', () => ({ refreshSession }))
vi.mock('./session', () => ({ refreshSession }))

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
    refreshSession.mockReset()
    store.token = 'access-token'
    fetchImpl.mockReset()
    vi.stubGlobal('fetch', fetchImpl)
  })

  it('refreshes and repeats once for an ordinary 401', async () => {
    respondWith({ status: 401 }, { status: 200, body: { ok: true } })
    refreshSession.mockResolvedValue({ kind: 'renewed', token: 'fresh-token' })
    const { api } = await import('./client')

    await expect(api('/v1/jobs')).resolves.toEqual({ ok: true })

    expect(refreshSession).toHaveBeenCalledTimes(1)
    expect(fetchImpl).toHaveBeenCalledTimes(2)
  })

  it('does not refresh or replay a 401 from the sign-in surface', async () => {
    // The precondition that makes this bite: a signed-in operator on /login
    // holds a valid refresh cookie, so the refresh would succeed and the replay
    // would land — two failed attempts recorded for one mistyped password, and
    // lockout after three tries instead of five.
    respondWith({ status: 401, body: { error: 'invalid_credentials' } })
    refreshSession.mockResolvedValue({ kind: 'renewed', token: 'fresh-token' })
    const { api, ApiError } = await import('./client')

    await expect(api('/v1/auth/login')).rejects.toBeInstanceOf(ApiError)

    expect(refreshSession).not.toHaveBeenCalled()
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

    expect(refreshSession).not.toHaveBeenCalled()
    expect(fetchImpl).toHaveBeenCalledTimes(1)
  })

  it.each([
    ['/v1/invitations/accept', 'redeeming an invitation'],
    ['/v1/password-reset/confirm', 'confirming a password reset'],
  ])('does not refresh-and-replay a 401 from %s', async (path) => {
    // Same shape as the sign-in surface and missed by the original prefix
    // match: the caller has no session by definition, so a 401 is the answer —
    // a wrong or expired token — rather than an expired access token. The
    // refresh cannot succeed and the replay sends the attempt twice
    // (issue #725).
    respondWith({ status: 401, body: { error: 'invalid_token' } })
    refreshSession.mockResolvedValue({ kind: 'renewed', token: 'fresh-token' })
    const { api } = await import('./client')

    await expect(api(path, { method: 'POST' })).rejects.toThrow()

    expect(refreshSession).not.toHaveBeenCalled()
    expect(fetchImpl).toHaveBeenCalledTimes(1)
  })

  it('still refreshes for an ordinary endpoint that happens to start similarly', async () => {
    // The exemption is a list of prefixes, so it is worth pinning that it does
    // not swallow the endpoints that *do* carry an access token.
    respondWith({ status: 401 }, { status: 200, body: { ok: true } })
    refreshSession.mockResolvedValue({ kind: 'renewed', token: 'fresh-token' })
    const { api } = await import('./client')

    await expect(api('/v1/invitations')).resolves.toEqual({ ok: true })

    expect(refreshSession).toHaveBeenCalledTimes(1)
  })

  it('leaves a non-401 alone', async () => {
    respondWith({ status: 403, body: { message: 'Forbidden' } })
    const { api } = await import('./client')

    await expect(api('/v1/jobs')).rejects.toThrow('Forbidden')

    expect(refreshSession).not.toHaveBeenCalled()
    expect(fetchImpl).toHaveBeenCalledTimes(1)
  })

  it('says "Session expired" only when the server ended the session', async () => {
    respondWith({ status: 401 })
    refreshSession.mockResolvedValue({ kind: 'ended' })
    const { api } = await import('./client')

    const caught = (await api('/v1/jobs').catch((error: unknown) => error)) as {
      status: number
      message: string
    }

    expect(caught.status).toBe(401)
    expect(caught.message).toBe('Session expired')
  })

  it('does not call an unreachable server an expired session', async () => {
    // Review 2026-10-24, U-3: a refresh that met a 502 during a restart left
    // the store signed in, and every list on the page said "Session expired".
    respondWith({ status: 401 })
    refreshSession.mockResolvedValue({ kind: 'unreachable' })
    const { api } = await import('./client')

    const caught = (await api('/v1/jobs').catch((error: unknown) => error)) as {
      status: number
      message: string
    }

    expect(caught.status).toBe(0)
    expect(caught.message).not.toMatch(/expired/i)
    expect(fetchImpl).toHaveBeenCalledTimes(1)
  })

  it('repeats with the newer token instead of refreshing again', async () => {
    // A 401 for the old token that lands after the single-flight refresh has
    // finished: refreshing again would rotate the refresh token a second time
    // for nothing (review 2026-10-24, U-3).
    let call = 0
    const sent: (string | null)[] = []
    fetchImpl.mockImplementation(async (request: Request | string, init?: RequestInit) => {
      const headers = new Headers(request instanceof Request ? request.headers : init?.headers)
      sent.push(headers.get('Authorization'))
      call += 1
      if (call === 1) {
        store.token = 'renewed-meanwhile'
        return new Response('{}', { status: 401, headers: { 'content-type': 'application/json' } })
      }
      return new Response('{"ok":true}', { status: 200, headers: { 'content-type': 'application/json' } })
    })
    const { api } = await import('./client')

    await expect(api('/v1/jobs')).resolves.toEqual({ ok: true })

    expect(refreshSession).not.toHaveBeenCalled()
    expect(sent).toEqual(['Bearer access-token', 'Bearer renewed-meanwhile'])
  })
})
