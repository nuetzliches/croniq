// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { apiFetch, apiPost } from './client'
import { useAuthStore } from '@/auth/store'

/**
 * The 401 path after #454.
 *
 * The access token is memory-only and lives an hour against a 7-day refresh
 * credential, so a mid-session 401 is an expired access token, not the end of
 * the session. `apiFetch` refreshes once and replays the request.
 */

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function authHeader(init: RequestInit | undefined): string | undefined {
  return (init?.headers as Record<string, string> | undefined)?.Authorization
}

beforeEach(() => {
  localStorage.clear()
  useAuthStore.setState({ token: 'stale-access', status: 'authenticated' })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('apiFetch on 401', () => {
  it('refreshes and replays the request with the new token', async () => {
    const fetchMock = vi
      .fn()
      // 1. the original call, with the stale token
      .mockResolvedValueOnce(jsonResponse(401, {}))
      // 2. the refresh
      .mockResolvedValueOnce(jsonResponse(200, { access_token: 'fresh-access' }))
      // 3. the replay
      .mockResolvedValueOnce(jsonResponse(200, { job_key: 'nightly' }))
    vi.stubGlobal('fetch', fetchMock)

    await expect(apiFetch('/v1/jobs/nightly')).resolves.toEqual({ job_key: 'nightly' })

    expect(fetchMock).toHaveBeenCalledTimes(3)
    expect(authHeader(fetchMock.mock.calls[0][1])).toBe('Bearer stale-access')
    expect(fetchMock.mock.calls[1][0]).toBe('/v1/auth/refresh')
    expect(authHeader(fetchMock.mock.calls[2][1])).toBe('Bearer fresh-access')
    expect(useAuthStore.getState().token).toBe('fresh-access')
  })

  it('stops after one replay when the request is genuinely unauthorized', async () => {
    // A superseded token generation, a deactivated user, a missing scope — a
    // second 401 after a good refresh is an answer, not staleness.
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse(401, {}))
      .mockResolvedValueOnce(jsonResponse(200, { access_token: 'fresh-access' }))
      .mockResolvedValueOnce(jsonResponse(401, {}))
    vi.stubGlobal('fetch', fetchMock)

    await expect(apiFetch('/v1/jobs')).rejects.toThrow('Unauthorized')
    expect(fetchMock).toHaveBeenCalledTimes(3)
  })

  it('clears the session when the refresh finds none', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse(401, {}))
      // Both the refresh and its one retry come back 401.
      .mockResolvedValue(jsonResponse(401, {}))
    vi.stubGlobal('fetch', fetchMock)

    await expect(apiFetch('/v1/jobs')).rejects.toThrow('Unauthorized')
    expect(useAuthStore.getState().status).toBe('anonymous')
  })

  it('never refreshes-and-replays a sign-in request', async () => {
    // A wrong password answers 401. Replaying it would count the attempt twice
    // against `failed_attempts` and the per-IP throttle, locking an account
    // after three tries instead of five — and no /v1/auth/* endpoint
    // authenticates with an access token in the first place.
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(401, {}))
    vi.stubGlobal('fetch', fetchMock)

    await expect(
      apiPost('/v1/auth/login', { username: 'admin', password: 'wrong' }),
    ).rejects.toThrow('Unauthorized')

    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls[0][0]).toBe('/v1/auth/login')
    // The session state is untouched: a failed login is not a lost session.
    expect(useAuthStore.getState().status).toBe('authenticated')
  })
})
