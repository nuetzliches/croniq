import { ApiError } from '@/api/error'

/**
 * What went wrong on a sign-in attempt, as far as the browser can tell.
 *
 * The distinction that matters is `unreachable` vs `server`: a transport
 * failure means the request never got an answer, while an HTTP error *response*
 * proves the server was reached, replied, and failed on its own. Collapsing
 * both into "cannot reach server" used to send operators to check the proxy,
 * DNS and container status while the server was healthy and answering — the
 * concrete case being a 500 from a TOTP secret that no longer decrypts, where
 * `/v1/auth/config` and `/health` both stay green (issues #410, #408).
 */
export type LoginFailureKind =
  | 'unreachable'
  | 'upstream'
  | 'server'
  | 'credentials'
  | 'inactive'
  | 'throttled'
  | 'unknown'

export interface LoginFailure {
  kind: LoginFailureKind
  /** Message to show the operator. */
  message: string
}

/** HTTP status of a failed request, when the failure carries one. */
function statusOf(err: unknown): number | undefined {
  if (err instanceof ApiError) return err.status
  // `apiFetch` throws a plain Error for 401 (it also clears the session), and
  // older call paths format the message as "<status>: <body>".
  if (err instanceof Error) {
    if (err.message === 'Unauthorized') return 401
    const m = /^(\d{3})[: ]/.exec(err.message)
    if (m) return Number(m[1])
  }
  return undefined
}

/**
 * Classify a `/v1/auth/login` failure. `hadCode` is true when the attempt
 * carried a TOTP or recovery code, which makes a 401 ambiguous between the
 * password and the code.
 */
export function classifyLoginFailure(err: unknown, hadCode = false): LoginFailure {
  // A `TypeError` from fetch is the only genuine reachability signal: DNS,
  // connection refused, TLS, CORS, offline.
  if (err instanceof TypeError) {
    return {
      kind: 'unreachable',
      message: 'Cannot reach server. Check that the Croniq backend is running.',
    }
  }

  const status = statusOf(err)

  if (status === 401) {
    return {
      kind: 'credentials',
      message: hadCode
        ? 'Sign-in failed. Check your password and code.'
        : 'Invalid credentials.',
    }
  }
  if (status === 403) {
    // A locked account answers 401 like any other bad credential, so that a
    // 403 cannot be used to probe which usernames exist (issue #428). What
    // is left here is a deactivated account, which is only reachable once
    // the password itself was correct.
    return {
      kind: 'inactive',
      message: 'Account is inactive. Contact an admin.',
    }
  }
  // Per-IP login throttle (issue #428). Distinct from a wrong password: no
  // number of retries will help until the window slides.
  if (status === 429) {
    return {
      kind: 'throttled',
      message: 'Too many sign-in attempts from this address. Wait a few minutes and try again.',
    }
  }
  // 502/503/504 come from a proxy or an upstream that isn't answering, so the
  // reachability framing is fair there — but name the layer.
  if (status === 502 || status === 503 || status === 504) {
    return {
      kind: 'upstream',
      message: `Server unavailable (HTTP ${status}). A proxy or the backend is not accepting requests — check that the Croniq backend is running.`,
    }
  }
  if (status !== undefined && status >= 500) {
    return {
      kind: 'server',
      message: `Server error during sign-in (HTTP ${status}). The backend is reachable but failed internally — check the server logs.`,
    }
  }
  return { kind: 'unknown', message: 'Login failed. Check your credentials.' }
}
