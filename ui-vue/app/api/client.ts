import { ofetch, type FetchOptions } from 'ofetch'
import { useAuthStore } from '~/stores/auth'
import { refreshAccessToken } from './session'

/**
 * The one HTTP client.
 *
 * Same-origin by construction — no configurable base URL. ADR-0001 makes the
 * dashboard same-origin with the API, and the deployments that serve this
 * bundle (`croniq-server --ui-dir`, the `croniq-ui` container behind a reverse
 * proxy) both satisfy that. A base-URL option would be an invitation to break
 * it quietly.
 */

/** Shape of the server's error envelope, where it sends one. */
interface ApiErrorBody {
  error?: string
  message?: string
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    /** The parsed body, when there was one — callers inspect `error` codes. */
    readonly body?: ApiErrorBody,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

function messageFor(status: number, body: unknown): string {
  const parsed = body as ApiErrorBody | undefined
  return parsed?.message ?? parsed?.error ?? `Request failed with ${status}`
}

const base = ofetch.create({
  retry: false,
  onRequest({ options }) {
    const token = useAuthStore().token
    if (token) {
      options.headers.set('Authorization', `Bearer ${token}`)
    }
  },
})

/**
 * Perform a request, refreshing once on a 401.
 *
 * The retry is here rather than in `ofetch`'s own `retry` option because the
 * two are not the same thing: ofetch retries the *same* request, while this
 * has to obtain a new credential first and only then repeat it — and only for
 * 401, and only once, so a genuinely revoked session fails fast instead of
 * looping against an answer that will not change.
 */
export async function api<T>(path: string, options: FetchOptions = {}): Promise<T> {
  try {
    return (await base<T>(path, options as never)) as T
  } catch (error) {
    const status = (error as { response?: { status?: number } }).response?.status
    if (status !== 401) {
      throw new ApiError(
        status ?? 0,
        messageFor(status ?? 0, (error as { data?: unknown }).data),
        (error as { data?: ApiErrorBody }).data,
      )
    }

    // 401: the access token lives an hour and the app outlives it. Refresh and
    // repeat once. `refreshAccessToken` is single-flight, so a page full of
    // simultaneous 401s produces one refresh.
    const token = await refreshAccessToken()
    if (!token) throw new ApiError(401, 'Session expired')
    try {
      return (await base<T>(path, options as never)) as T
    } catch (retryError) {
      const retryStatus = (retryError as { response?: { status?: number } }).response?.status
      throw new ApiError(
        retryStatus ?? 0,
        messageFor(retryStatus ?? 0, (retryError as { data?: unknown }).data),
        (retryError as { data?: ApiErrorBody }).data,
      )
    }
  }
}

export const apiGet = <T>(path: string, query?: FetchOptions['query']) =>
  api<T>(path, { method: 'GET', query })

export const apiPost = <T>(path: string, body?: unknown) =>
  api<T>(path, { method: 'POST', body: body as Record<string, unknown> })

export const apiPut = <T>(path: string, body?: unknown) =>
  api<T>(path, { method: 'PUT', body: body as Record<string, unknown> })

export const apiDelete = <T>(path: string) => api<T>(path, { method: 'DELETE' })
