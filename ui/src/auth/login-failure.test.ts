import { describe, expect, it } from 'vitest'
import { ApiError } from '@/api/error'
import { classifyLoginFailure } from './login-failure'

/** The failure `apiFetch` produces for a non-OK response. */
function apiError(status: number, body = '') {
  return new ApiError(status, body)
}

describe('classifyLoginFailure', () => {
  it('reports a transport failure as unreachable', () => {
    // `fetch` rejects with a TypeError on DNS / refused / offline — the only
    // case where "is the backend running?" is the right question.
    const f = classifyLoginFailure(new TypeError('Failed to fetch'))
    expect(f.kind).toBe('unreachable')
    expect(f.message).toMatch(/cannot reach server/i)
  })

  it('does not report a 500 as unreachable (#410)', () => {
    const f = classifyLoginFailure(apiError(500))
    expect(f.kind).toBe('server')
    expect(f.message).toContain('HTTP 500')
    expect(f.message).toMatch(/server logs/i)
    // The regression: this is what sent operators to check proxy/DNS/container
    // while the server was healthy and answering.
    expect(f.message).not.toMatch(/cannot reach server/i)
  })

  it.each([502, 503, 504])('frames %i as an unavailable upstream', (status) => {
    // Proxy / upstream-not-answering: reachability wording is fair here, but
    // it names the layer instead of implying the process is down.
    const f = classifyLoginFailure(apiError(status))
    expect(f.kind).toBe('upstream')
    expect(f.message).toContain(`HTTP ${status}`)
  })

  it('keeps the credential message for a 401', () => {
    const f = classifyLoginFailure(apiError(401))
    expect(f.kind).toBe('credentials')
    expect(f.message).toBe('Invalid credentials.')
  })

  it("treats apiFetch's bare Unauthorized error as a 401", () => {
    // `apiFetch` clears the session and throws `new Error('Unauthorized')` for
    // 401 instead of an ApiError, so status has to be recovered from that too —
    // otherwise every wrong password fell through to the generic message.
    const f = classifyLoginFailure(new Error('Unauthorized'))
    expect(f.kind).toBe('credentials')
  })

  it('names both factors when a code was submitted', () => {
    // A bare 401 cannot say whether the password or the code was wrong.
    const f = classifyLoginFailure(apiError(401), true)
    expect(f.kind).toBe('credentials')
    expect(f.message).toMatch(/password and code/i)
  })

  it('reports a 403 as an inactive account', () => {
    const f = classifyLoginFailure(apiError(403))
    expect(f.kind).toBe('inactive')
    // A locked account is a 401 now, so the message must not promise to
    // explain lockouts (issue #428).
    expect(f.message).not.toMatch(/locked/i)
  })

  it('reports a 429 as throttled rather than bad credentials', () => {
    // The per-IP login limiter: retrying immediately cannot help, so this
    // must not read as "wrong password".
    const f = classifyLoginFailure(apiError(429))
    expect(f.kind).toBe('throttled')
    expect(f.message).toMatch(/too many sign-in attempts/i)
  })

  it('parses the legacy "<status>: <body>" message format', () => {
    expect(classifyLoginFailure(new Error('500: boom')).kind).toBe('server')
    expect(classifyLoginFailure(new Error('403: nope')).kind).toBe('inactive')
  })

  it('falls back to a generic message for anything unrecognised', () => {
    const f = classifyLoginFailure(new Error('something odd'))
    expect(f.kind).toBe('unknown')
    expect(f.message).toBe('Login failed. Check your credentials.')
  })
})
