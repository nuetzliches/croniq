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

/**
 * What a refresh attempt established.
 *
 * Three answers, not two (review 2026-10-24, U-1/U-3). `null` used to stand for
 * both "the session is over" and "the server could not be asked" — a network
 * error, a 502 from the proxy during a restart, a 200 that was not JSON. The
 * SSE streams took the second for the first and ended for good, leaving the
 * page signed in behind stale data; every list said "Session expired" to a
 * user who was still signed in. Only `ended` clears the store.
 */
export type RefreshOutcome =
  | { kind: 'renewed'; token: string }
  /** The server rejected the session; the store has been cleared. */
  | { kind: 'ended' }
  /** No verdict: the server could not be reached or answered nonsense. */
  | { kind: 'unreachable' }

let inFlight: Promise<RefreshOutcome> | null = null

/**
 * Exchange the refresh cookie for a new access token.
 *
 * Single-flight: a page full of queries hitting 401 at once must produce one
 * refresh, not one per request. Beyond the wasted round-trips, the refresh
 * token **rotates** — parallel calls would each revoke the previous one's
 * successor, and the last one standing would sign the user out.
 */
export function refreshSession(): Promise<RefreshOutcome> {
  inFlight ??= runRefresh(true).finally(() => {
    inFlight = null
  })
  return inFlight
}

async function runRefresh(allowRetry: boolean): Promise<RefreshOutcome> {
  let response: Response
  try {
    response = await fetch('/v1/auth/refresh', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: '{}',
    })
  } catch {
    // A network failure is not an auth failure, so it does not decide that the
    // session is over. The caller decides what "no answer" means for it: the
    // bootstrap has no token and signs in again, a stream backs off and asks
    // later.
    return { kind: 'unreachable' }
  }

  if (response.status === 401) {
    // Two tabs share one cookie jar, so a tab whose refresh raced another
    // tab's rotation sees a 401 for a token that has already been replaced.
    // Retrying once reads whatever `Set-Cookie` landed meanwhile and usually
    // succeeds.
    //
    // But only when a credential was actually presented. The server marks the
    // other case — nothing sent at all — as `no_session` (#621), because the
    // cookie is `HttpOnly` and this page cannot look for itself. Without that
    // marker every first-ever visitor, and everyone whose seven-day refresh
    // token has expired, paid a second round trip that could not possibly
    // succeed.
    if (allowRetry && !(await isNoSession(response))) return runRefresh(false)
    useAuthStore().clear()
    return { kind: 'ended' }
  }
  // A 5xx is the proxy or a restarting server, not a verdict on the session.
  if (!response.ok) return { kind: 'unreachable' }

  // A 200 is only an answer if it carries a token. A proxy that falls back to
  // `index.html` for unknown paths, or a captive portal, answers 200 with HTML:
  // an unguarded `json()` threw out of the bootstrap and left the app on a
  // blank page, and JSON without `access_token` marked the store
  // `authenticated` with no token in it (review 2026-10-24, U-2).
  const token = await readAccessToken(response)
  if (token === null) return { kind: 'unreachable' }
  useAuthStore().setToken(token)
  return { kind: 'renewed', token }
}

async function readAccessToken(response: Response): Promise<string | null> {
  try {
    const body = (await response.json()) as { access_token?: unknown } | null
    const token = body?.access_token
    return typeof token === 'string' && token !== '' ? token : null
  } catch {
    return null
  }
}

/**
 * Did the server say no credential was presented, rather than rejected one?
 *
 * Tolerant on purpose: an older server answers 401 with no body at all, and a
 * proxy may replace the body with something else entirely. Anything this
 * cannot read is treated as "a credential was presented", which keeps the
 * retry — the conservative direction, since the cost of a wrong guess here is
 * one extra request rather than a session that fails to come back.
 */
async function isNoSession(response: Response): Promise<boolean> {
  try {
    const body = (await response.clone().json()) as { error?: string }
    return body?.error === 'no_session'
  } catch {
    return false
  }
}

/**
 * Keys the React dashboard kept tokens under, before #454.
 *
 * That issue moved the refresh credential into an `HttpOnly` cookie precisely
 * so a script on this origin could not read it. The React tree cleaned these up
 * on every login, refresh failure and logout; this one never mentioned them, so
 * a browser that used the old dashboard kept a readable refresh token
 * indefinitely after the upgrade (issue #719).
 *
 * It has very likely expired — they lasted seven days — but "probably expired"
 * is not the property #454 was after.
 */
const LEGACY_TOKEN_KEYS = ['croniq_token', 'croniq_refresh']

/** Remove them, once, wherever storage is reachable. */
export function forgetLegacyTokens() {
  for (const key of LEGACY_TOKEN_KEYS) {
    try {
      localStorage.removeItem(key)
    } catch {
      // Private mode, or storage disabled. Nothing to remove there either.
    }
  }
}

/**
 * Establish session state at app start.
 *
 * Resolves once the store has left `'unknown'`. A first-ever visit gets a 401
 * here — the expected answer, not an error.
 */
export async function bootstrap(): Promise<void> {
  // Never rejects, and never leaves the store `unknown`: the router guard waits
  // for it to leave that state, so anything else is a blank page with no
  // diagnosis (review 2026-10-24, U-2). With no token in hand, "the server
  // could not be asked" means the same as "no session" here — sign in.
  try {
    const outcome = await refreshSession()
    if (outcome.kind !== 'renewed') useAuthStore().clear()
  } catch {
    useAuthStore().clear()
  }
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
  // `deliberate`: the auth watch must not chase this with a redirect carrying
  // a `next` back to the page just left. See the store.
  useAuthStore().clear({ deliberate: true })
}
