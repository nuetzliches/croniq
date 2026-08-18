// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { acceptTokens, bootstrap, logout, refreshAccessToken } from './session'
import { useAuthStore } from './store'

/**
 * Session handling after #454: nothing persistent, and a 401 is recoverable.
 *
 * jsdom for `localStorage` — the assertion that it stays untouched is the
 * regression guard for the whole issue, so the test needs a real one to watch.
 * These run in same-origin (cookie) mode, which is what `REFRESH_COOKIE_MODE`
 * evaluates to whenever `VITE_API_URL` is unset — as it is under vitest.
 */

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

beforeEach(() => {
  localStorage.clear()
  useAuthStore.setState({ token: null, status: 'unknown' })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('acceptTokens', () => {
  it('keeps the access token in memory and writes nothing to localStorage', () => {
    acceptTokens({ access_token: 'access-1' })

    expect(useAuthStore.getState().token).toBe('access-1')
    expect(useAuthStore.getState().status).toBe('authenticated')
    expect(localStorage.length).toBe(0)
  })

  it('refuses to persist a refresh token even if the server sends one', () => {
    // Belt and braces: in cookie mode the server omits the field, but a stray
    // one must not become the durable, XSS-readable credential #454 removes.
    acceptTokens({ access_token: 'access-1', refresh_token: 'refresh-1' })

    expect(localStorage.getItem('croniq_refresh')).toBeNull()
    expect(localStorage.length).toBe(0)
  })

  it('clears tokens left behind by a pre-#454 build', () => {
    localStorage.setItem('croniq_token', 'old-access')
    localStorage.setItem('croniq_refresh', 'old-refresh')

    acceptTokens({ access_token: 'access-1' })

    expect(localStorage.getItem('croniq_token')).toBeNull()
    expect(localStorage.getItem('croniq_refresh')).toBeNull()
  })
})

describe('refreshAccessToken', () => {
  it('sends an empty body — the cookie is the credential', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(jsonResponse(200, { access_token: 'fresh' }))
    vi.stubGlobal('fetch', fetchMock)

    await expect(refreshAccessToken()).resolves.toBe('fresh')

    const [url, init] = fetchMock.mock.calls[0]
    expect(url).toBe('/v1/auth/refresh')
    expect(init.method).toBe('POST')
    expect(JSON.parse(init.body)).toEqual({})
    expect(useAuthStore.getState().token).toBe('fresh')
  })

  it('coalesces concurrent callers into one request', async () => {
    // A page full of queries hitting 401 at once must not fire N refreshes:
    // the token rotates, so the later ones would revoke the winner's successor.
    const fetchMock = vi
      .fn()
      .mockResolvedValue(jsonResponse(200, { access_token: 'fresh' }))
    vi.stubGlobal('fetch', fetchMock)

    const results = await Promise.all([
      refreshAccessToken(),
      refreshAccessToken(),
      refreshAccessToken(),
    ])

    expect(results).toEqual(['fresh', 'fresh', 'fresh'])
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('starts a fresh request once the previous one settled', async () => {
    // A new Response per call: a body can only be read once.
    const fetchMock = vi
      .fn()
      .mockImplementation(async () => jsonResponse(200, { access_token: 'fresh' }))
    vi.stubGlobal('fetch', fetchMock)

    await refreshAccessToken()
    await refreshAccessToken()

    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('retries a 401 once, for the two-tab cookie-rotation race', async () => {
    // Two tabs share one cookie jar. A tab whose refresh raced another tab's
    // rotation gets a 401 for a token that was already replaced; the retry
    // reads whatever Set-Cookie landed meanwhile.
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse(401, {}))
      .mockResolvedValueOnce(jsonResponse(200, { access_token: 'fresh' }))
    vi.stubGlobal('fetch', fetchMock)

    await expect(refreshAccessToken()).resolves.toBe('fresh')
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(useAuthStore.getState().status).toBe('authenticated')
  })

  it('gives up and clears the session after a second 401', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(401, {}))
    vi.stubGlobal('fetch', fetchMock)

    await expect(refreshAccessToken()).resolves.toBeNull()
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(useAuthStore.getState().status).toBe('anonymous')
    expect(useAuthStore.getState().token).toBeNull()
  })

  it('leaves the verdict to the caller on a network failure', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('offline')))

    await expect(refreshAccessToken()).resolves.toBeNull()
    // Not 'anonymous': a transport failure is not the server saying no.
    expect(useAuthStore.getState().status).toBe('unknown')
  })
})

describe('bootstrap', () => {
  it('turns the cookie into a session on reload', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(jsonResponse(200, { access_token: 'fresh' })),
    )

    await bootstrap()

    expect(useAuthStore.getState().status).toBe('authenticated')
    expect(useAuthStore.getState().token).toBe('fresh')
  })

  it('settles on anonymous when there is no session to recover', async () => {
    // A first-ever visit. The store must leave 'unknown' either way, or
    // ProtectedRoute spins forever.
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse(401, {})))

    await bootstrap()

    expect(useAuthStore.getState().status).toBe('anonymous')
  })

  it('settles on anonymous when the server is unreachable', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('offline')))

    await bootstrap()

    expect(useAuthStore.getState().status).toBe('anonymous')
  })
})

describe('logout', () => {
  it('revokes server-side before clearing local state', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 204 }))
    vi.stubGlobal('fetch', fetchMock)
    acceptTokens({ access_token: 'access-1' })

    await logout()

    const [url, init] = fetchMock.mock.calls[0]
    expect(url).toBe('/v1/auth/logout')
    // Empty body: the server reads the cookie. Sending a token we do not have
    // would be the only alternative, and we deliberately do not have one.
    expect(JSON.parse(init.body)).toEqual({})
    expect(useAuthStore.getState().status).toBe('anonymous')
    expect(useAuthStore.getState().token).toBeNull()
  })

  it('clears the session even when the revoke call fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('offline')))
    acceptTokens({ access_token: 'access-1' })

    await logout()

    expect(useAuthStore.getState().status).toBe('anonymous')
    expect(localStorage.length).toBe(0)
  })
})
