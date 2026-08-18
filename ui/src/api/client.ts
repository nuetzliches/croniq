import { useAuthStore } from '@/auth/store'
import { refreshAccessToken } from '@/auth/session'
import { API_BASE } from './base'
import { ApiError } from './error'

// Re-exported so `import { ApiError } from '@/api/client'` keeps working.
export { ApiError }

/**
 * Endpoints where a 401 is an *answer*, not an expired access token.
 *
 * `/v1/auth/*` is the sign-in surface: a wrong password answers 401, and
 * refreshing-and-retrying there would (a) fire a pointless refresh and (b)
 * replay the login — double-counting it against `failed_attempts` and the
 * per-IP throttle, so five wrong passwords would lock the account after three
 * attempts. None of these endpoints authenticate with an access token anyway.
 */
function isAuthEndpoint(path: string): boolean {
  return path.startsWith('/v1/auth/')
}

/**
 * Send a request with the current access token, refreshing once on 401.
 *
 * Since #454 the access token is memory-only and short-lived (1 hour) while the
 * refresh credential lasts 7 days, so a 401 mid-session is the *expected* end
 * of an access token rather than the end of the session. Refresh, retry once,
 * and only give up — clearing the session — if the refresh itself finds none.
 *
 * The retry is deliberately capped at one: a second 401 after a successful
 * refresh means the request is genuinely unauthorized (superseded token
 * generation, deactivated user, insufficient scope), not stale.
 */
async function authedFetch(path: string, init: RequestInit): Promise<Response> {
  const send = (token: string | null) =>
    fetch(`${API_BASE}${path}`, {
      ...init,
      headers: {
        ...init.headers,
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
    })

  const res = await send(useAuthStore.getState().token)
  if (res.status !== 401 || isAuthEndpoint(path)) return res

  const refreshed = await refreshAccessToken()
  if (!refreshed) {
    // `refreshAccessToken` already cleared the session when the refresh
    // credential is gone; this covers the paths where it returned null without
    // deciding (no stored token, network error). ProtectedRoute takes over.
    useAuthStore.getState().clear()
    return res
  }
  return send(refreshed)
}

export async function apiFetch<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await authedFetch(path, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
  })
  if (res.status === 401) {
    throw new Error('Unauthorized')
  }
  if (!res.ok) {
    const raw = await res.text()
    let parsed: unknown
    try {
      parsed = JSON.parse(raw)
    } catch {
      parsed = undefined
    }
    throw new ApiError(res.status, raw, parsed)
  }
  // 204 No Content + 205 Reset Content carry no body — calling res.json()
  // would throw "Unexpected end of JSON input". Caller's <T> is typically
  // `void` for these endpoints; cast to satisfy the signature.
  if (res.status === 204 || res.status === 205) {
    return undefined as unknown as T
  }
  return res.json()
}

export async function apiDelete(path: string): Promise<void> {
  const res = await authedFetch(path, { method: 'DELETE' })
  if (res.status === 401) {
    throw new Error('Unauthorized')
  }
  if (!res.ok) {
    const body = await res.text()
    throw new Error(`${res.status}: ${body}`)
  }
}

export async function apiPost<T>(path: string, body: unknown): Promise<T> {
  return apiFetch<T>(path, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function apiPut<T>(path: string, body: unknown): Promise<T> {
  return apiFetch<T>(path, {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}
