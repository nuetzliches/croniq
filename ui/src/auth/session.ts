import { API_BASE, REFRESH_COOKIE_MODE } from '@/api/base'
import { useAuthStore } from './store'

/**
 * Session lifecycle: accept tokens, refresh, sign out (issue #454).
 *
 * Kept out of both the store and the components because it is the one piece of
 * auth logic worth testing on its own — the repo has no React testing library,
 * so anything embedded in a component is untestable (same reasoning as
 * `login-failure.ts`).
 *
 * ## What changed with #454
 *
 * The access token is memory-only and the refresh token is an `HttpOnly`
 * cookie the browser attaches to `/v1/auth/refresh` by itself. Two
 * consequences shape everything here:
 *
 * 1. **A reload starts with no access token.** [`bootstrap`] trades the cookie
 *    for one before the router decides whether to show the login page.
 * 2. **The access token now actually expires mid-session.** Before this, a 401
 *    logged the user out — with a 1-hour access TTL against a 7-day refresh
 *    TTL, that meant being kicked out hourly. `api/client.ts` refreshes and
 *    retries the request instead.
 *
 * In a cross-origin (`VITE_API_URL`) build there is no cookie — see
 * [`REFRESH_COOKIE_MODE`] — so the refresh token is kept in `localStorage` as
 * before and passed in the request body.
 */

/** `localStorage` key, used by the cross-origin fallback path only. */
const LEGACY_REFRESH_KEY = 'croniq_refresh'
/** Retired by #454. Removed on sight so stale copies do not linger. */
const LEGACY_TOKEN_KEY = 'croniq_token'

interface TokenReply {
  access_token: string
  /** Absent in cookie mode — that omission is the point. */
  refresh_token?: string
}

/**
 * Store what a login or refresh reply handed back.
 *
 * In cookie mode that is only the access token; the refresh token arrived as a
 * `Set-Cookie` the browser has already applied and JavaScript cannot read. In
 * cross-origin mode the refresh token is persisted, because a reload has
 * nothing else to recover the session from.
 */
export function acceptTokens(reply: TokenReply): void {
  if (REFRESH_COOKIE_MODE) {
    // A cookie-mode reply must not carry a refresh token. If a server sends
    // one regardless, drop it rather than persisting it.
    forgetStoredTokens()
  } else if (reply.refresh_token) {
    localStorage.setItem(LEGACY_REFRESH_KEY, reply.refresh_token)
  }
  useAuthStore.getState().setToken(reply.access_token)
}

function forgetStoredTokens(): void {
  localStorage.removeItem(LEGACY_REFRESH_KEY)
  localStorage.removeItem(LEGACY_TOKEN_KEY)
}

/**
 * Exchange the refresh credential for a new access token, or `null` if there
 * is no usable session.
 *
 * Single-flight: a page full of queries hitting 401 at once must produce one
 * refresh, not one per request. Beyond the wasted round-trips, the refresh
 * token *rotates* — parallel calls would each revoke the previous one's
 * successor and the last one standing would log the user out.
 */
let inFlight: Promise<string | null> | null = null

export function refreshAccessToken(): Promise<string | null> {
  inFlight ??= runRefresh(true).finally(() => {
    inFlight = null
  })
  return inFlight
}

async function runRefresh(allowRetry: boolean): Promise<string | null> {
  const body: Record<string, string> = {}
  if (!REFRESH_COOKIE_MODE) {
    const stored = localStorage.getItem(LEGACY_REFRESH_KEY)
    if (!stored) return null
    body.refresh_token = stored
  }

  let res: Response
  try {
    res = await fetch(`${API_BASE}/v1/auth/refresh`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
  } catch {
    // Network failure, not an auth failure — so this does not itself decide
    // that the session is over. It reports "no token" and leaves the verdict
    // to the caller, both of which currently fall back to signing in again:
    // with no access token in hand there is nothing else the app can do.
    return null
  }

  if (res.status === 401) {
    // Once, and only in cookie mode: two tabs share one cookie jar, so a tab
    // whose refresh raced another tab's rotation sees a 401 for a token that
    // has already been replaced. The retry reads whatever `Set-Cookie` landed
    // meanwhile and usually succeeds. In body mode the stored token is all
    // there is, so a retry would send the identical value.
    if (REFRESH_COOKIE_MODE && allowRetry) return runRefresh(false)
    forgetStoredTokens()
    useAuthStore.getState().clear()
    return null
  }
  if (!res.ok) return null

  const reply = (await res.json()) as TokenReply
  acceptTokens(reply)
  return reply.access_token
}

/**
 * Establish session state at app start.
 *
 * Resolves once the store has left `'unknown'`, which is what `ProtectedRoute`
 * waits for. A first-ever visit gets a 401 here — the expected answer, not an
 * error.
 */
export async function bootstrap(): Promise<void> {
  const token = await refreshAccessToken()
  if (!token) useAuthStore.getState().clear()
}

/**
 * Sign out: revoke the refresh token server-side, then drop local state.
 *
 * The server call is what makes logout mean anything — the refresh token is
 * good for 7 days, and clearing a cookie in one browser does not revoke it.
 * A failure is deliberately ignored: the user asked to be signed out, so the
 * local session goes either way.
 */
export async function logout(): Promise<void> {
  const stored = REFRESH_COOKIE_MODE ? null : localStorage.getItem(LEGACY_REFRESH_KEY)
  try {
    await fetch(`${API_BASE}/v1/auth/logout`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(stored ? { refresh_token: stored } : {}),
    })
  } catch {
    // Offline logout still clears the browser.
  }
  forgetStoredTokens()
  useAuthStore.getState().clear()
}
