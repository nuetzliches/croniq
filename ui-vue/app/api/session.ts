import { useAuthStore } from '~/stores/auth'

/**
 * Session lifecycle: refresh, bootstrap, logout.
 *
 * Ported from the React tree's `auth/session.ts` with its semantics intact —
 * the single-flight guard and the one-shot retry below are not stylistic, they
 * are consequences of how the server behaves, and re-deriving them from
 * scratch is how a rebuild loses hard-won behaviour.
 *
 * One simplification: the React version supports a cross-origin build where
 * the refresh token lives in `localStorage`. This tree is same-origin only
 * (see vite.config.ts), so the cookie path is the only path and the
 * `localStorage` fallback is gone rather than carried along unused.
 */

interface TokenReply {
  access_token: string
}

let inFlight: Promise<string | null> | null = null

/**
 * Exchange the refresh cookie for a new access token, or `null` when there is
 * no usable session.
 *
 * Single-flight: a page full of queries hitting 401 at once must produce one
 * refresh, not one per request. Beyond the wasted round-trips, the refresh
 * token **rotates** — parallel calls would each revoke the previous one's
 * successor, and the last one standing would sign the user out.
 */
export function refreshAccessToken(): Promise<string | null> {
  inFlight ??= runRefresh(true).finally(() => {
    inFlight = null
  })
  return inFlight
}

async function runRefresh(allowRetry: boolean): Promise<string | null> {
  let response: Response
  try {
    response = await fetch('/v1/auth/refresh', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: '{}',
    })
  } catch {
    // A network failure is not an auth failure, so it does not decide that the
    // session is over. It reports "no token" and leaves the verdict to the
    // caller — which, with no access token in hand, means signing in again.
    return null
  }

  if (response.status === 401) {
    // Once. Two tabs share one cookie jar, so a tab whose refresh raced
    // another tab's rotation sees a 401 for a token that has already been
    // replaced; the retry reads whatever `Set-Cookie` landed meanwhile and
    // usually succeeds.
    if (allowRetry) return runRefresh(false)
    useAuthStore().clear()
    return null
  }
  if (!response.ok) return null

  const reply = (await response.json()) as TokenReply
  useAuthStore().setToken(reply.access_token)
  return reply.access_token
}

/**
 * Establish session state at app start.
 *
 * Resolves once the store has left `'unknown'`. A first-ever visit gets a 401
 * here — the expected answer, not an error.
 */
export async function bootstrap(): Promise<void> {
  const token = await refreshAccessToken()
  if (!token) useAuthStore().clear()
}

/**
 * Sign out: revoke server-side, then drop local state.
 *
 * The server call is what makes logout mean anything — the refresh token is
 * good for seven days, and clearing a cookie in one browser does not revoke
 * it. A failure is deliberately ignored: the user asked to be signed out, so
 * the local session goes either way.
 */
export async function logout(): Promise<void> {
  try {
    await fetch('/v1/auth/logout', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: '{}',
    })
  } catch {
    // ignored on purpose — see above
  }
  useAuthStore().clear()
}
