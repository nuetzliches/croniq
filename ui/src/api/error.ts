/**
 * Error thrown by the API helpers on a non-OK response. Carries the HTTP status
 * and the parsed JSON body (when the response was JSON) so callers can branch
 * on structured errors (e.g. the 409 stale-replay guard). The `message` keeps
 * the historical `"<status>: <text>"` format so existing toasts don't regress.
 *
 * Deliberately its own module: `client.ts` reaches into the auth store (and
 * therefore `localStorage`) at import time, so pure logic that only needs to
 * classify a failure — see `auth/login-failure.ts` — can import this without
 * pulling a DOM dependency into plain unit tests.
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
