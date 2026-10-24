import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useAuthStore } from '~/stores/auth'
import { bootstrap, refreshSession } from './session'

/**
 * The refresh is where a session survives or ends, and it had no test of its
 * own (review 2026-10-24, Testlücken). What matters is which answers end the
 * session — only a 401 does — and which merely mean "could not ask": a network
 * error, a 5xx, a 200 that is not the JSON it should be (U-1, U-2).
 */

const fetchMock = vi.fn()

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })

function respondWith(...responses: Array<Response | Error>) {
  let call = 0
  fetchMock.mockImplementation(async () => {
    const next = responses[Math.min(call, responses.length - 1)]
    call += 1
    if (next instanceof Error) throw next
    return next.clone()
  })
}

describe('refreshSession', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  it('stores the renewed token', async () => {
    respondWith(json(200, { access_token: 'fresh' }))

    await expect(refreshSession()).resolves.toEqual({ kind: 'renewed', token: 'fresh' })
    expect(useAuthStore().token).toBe('fresh')
    expect(useAuthStore().status).toBe('authenticated')
  })

  it('retries a rejected credential once, for the tab that lost a rotation race', async () => {
    respondWith(json(401, { error: 'invalid_token' }), json(200, { access_token: 'fresh' }))

    await expect(refreshSession()).resolves.toEqual({ kind: 'renewed', token: 'fresh' })
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('ends the session when the retry is rejected too, and stops there', async () => {
    useAuthStore().setToken('old')
    respondWith(json(401, { error: 'invalid_token' }))

    await expect(refreshSession()).resolves.toEqual({ kind: 'ended' })
    expect(fetchMock).toHaveBeenCalledTimes(2)
    expect(useAuthStore().status).toBe('anonymous')
    expect(useAuthStore().token).toBeNull()
  })

  it('does not retry when no credential was presented at all', async () => {
    respondWith(json(401, { error: 'no_session' }))

    await expect(refreshSession()).resolves.toEqual({ kind: 'ended' })
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(useAuthStore().status).toBe('anonymous')
  })

  it('retries a 401 without a readable body, the conservative direction', async () => {
    respondWith(new Response(null, { status: 401 }), json(200, { access_token: 'fresh' }))

    await expect(refreshSession()).resolves.toMatchObject({ kind: 'renewed' })
    expect(fetchMock).toHaveBeenCalledTimes(2)
  })

  it('treats a network error as no verdict and keeps the session', async () => {
    useAuthStore().setToken('still-mine')
    respondWith(new TypeError('Failed to fetch'))

    await expect(refreshSession()).resolves.toEqual({ kind: 'unreachable' })
    expect(useAuthStore().status).toBe('authenticated')
    expect(useAuthStore().token).toBe('still-mine')
  })

  it('treats a 5xx as no verdict and keeps the session', async () => {
    useAuthStore().setToken('still-mine')
    respondWith(new Response('Bad Gateway', { status: 502 }))

    await expect(refreshSession()).resolves.toEqual({ kind: 'unreachable' })
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(useAuthStore().status).toBe('authenticated')
  })

  it('does not throw on a 200 that is not JSON', async () => {
    // A proxy falling back to index.html for /v1/auth/refresh, or a captive
    // portal. The unguarded `json()` threw out of the bootstrap (U-2).
    respondWith(
      new Response('<!doctype html><html></html>', {
        status: 200,
        headers: { 'content-type': 'text/html' },
      }),
    )

    await expect(refreshSession()).resolves.toEqual({ kind: 'unreachable' })
    expect(useAuthStore().status).toBe('unknown')
  })

  it('does not mark the store authenticated for JSON without a token', async () => {
    respondWith(json(200, { something: 'else' }))

    await expect(refreshSession()).resolves.toEqual({ kind: 'unreachable' })
    expect(useAuthStore().status).not.toBe('authenticated')
    expect(useAuthStore().token).toBeNull()
  })

  it('makes one request for concurrent callers', async () => {
    respondWith(json(200, { access_token: 'fresh' }))

    const [a, b] = await Promise.all([refreshSession(), refreshSession()])

    expect(a).toEqual(b)
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })
})

describe('bootstrap', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  it('signs in from the refresh cookie', async () => {
    respondWith(json(200, { access_token: 'fresh' }))

    await bootstrap()

    expect(useAuthStore().status).toBe('authenticated')
  })

  it('leaves `unknown` for a first-ever visit', async () => {
    respondWith(json(401, { error: 'no_session' }))

    await bootstrap()

    expect(useAuthStore().status).toBe('anonymous')
  })

  it.each([
    ['a network error', () => new TypeError('Failed to fetch')],
    ['a 503', () => new Response(null, { status: 503 })],
    [
      'a 200 that is not JSON',
      () => new Response('<html></html>', { status: 200, headers: { 'content-type': 'text/html' } }),
    ],
  ])('resolves and leaves `unknown` on %s', async (_label, make) => {
    // The router guard waits for the store to leave `unknown`; a bootstrap
    // that rejected or stayed there was a blank page with no diagnosis (U-2).
    respondWith(make())

    await expect(bootstrap()).resolves.toBeUndefined()

    expect(useAuthStore().status).toBe('anonymous')
  })

  it('resolves even if the refresh itself throws', async () => {
    fetchMock.mockImplementation(() => {
      throw new Error('synchronous failure')
    })

    await expect(bootstrap()).resolves.toBeUndefined()
    expect(useAuthStore().status).toBe('anonymous')
  })
})
