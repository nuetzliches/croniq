import { useAuthStore } from '@/auth/store'

// Default to same-origin (relative URLs) so the UI works unchanged in any
// deployment where croniq-server serves both the UI and the API — which is
// the standard setup, including the official Docker image. For local dev
// (npm run dev on :5173) the Vite dev-server proxies /v1, /health, /metrics
// to http://localhost:4000 — see vite.config.ts. Override VITE_API_URL at
// build time only when UI and API live on different origins.
const BASE = import.meta.env.VITE_API_URL ?? ''

/**
 * Error thrown by API helpers on a non-OK response. Carries the HTTP status
 * and the parsed JSON body (when the response was JSON) so callers can branch
 * on structured errors (e.g. the 409 stale-replay guard). The `message` keeps
 * the historical `"<status>: <text>"` format so existing toasts don't regress.
 */
export class ApiError extends Error {
  status: number
  body?: unknown
  constructor(status: number, rawBody: string, body?: unknown) {
    super(`${status}: ${rawBody}`)
    this.name = 'ApiError'
    this.status = status
    this.body = body
  }
}

export async function apiFetch<T>(path: string, options?: RequestInit): Promise<T> {
  const token = useAuthStore.getState().token
  const res = await fetch(`${BASE}${path}`, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...options?.headers,
    },
  })
  if (res.status === 401) {
    useAuthStore.getState().logout()
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
  const token = useAuthStore.getState().token
  const res = await fetch(`${BASE}${path}`, {
    method: 'DELETE',
    headers: token ? { Authorization: `Bearer ${token}` } : {},
  })
  if (res.status === 401) {
    useAuthStore.getState().logout()
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
