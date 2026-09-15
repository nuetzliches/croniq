// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * Credentials the pre-#454 dashboard left in `localStorage`.
 *
 * #454 moved the refresh token into an `HttpOnly` cookie precisely so a script
 * on this origin could not read it. Both dashboards were served from the same
 * origin, so a browser that used the old one still holds `croniq_refresh` —
 * and the React tree removed both keys on every login, refresh failure and
 * logout, while this one never mentioned them (issue #719).
 *
 * The token has very likely expired; they lasted seven days. "Probably
 * expired" is not the property #454 was after.
 */

vi.mock('~/stores/auth', () => ({
  useAuthStore: () => ({ token: null, isAuthenticated: false, setToken: vi.fn(), clear: vi.fn() }),
}))

describe('forgetLegacyTokens', () => {
  beforeEach(() => {
    localStorage.clear()
    vi.resetModules()
  })

  it('removes both keys the React dashboard used', async () => {
    localStorage.setItem('croniq_token', 'an-old-access-token')
    localStorage.setItem('croniq_refresh', 'an-old-refresh-token')

    const { forgetLegacyTokens } = await import('./session')
    forgetLegacyTokens()

    expect(localStorage.getItem('croniq_token')).toBeNull()
    expect(localStorage.getItem('croniq_refresh')).toBeNull()
  })

  it('leaves this dashboard’s own preferences alone', async () => {
    // Same namespace, same prefix, and they are deliberately carried across
    // the cutover (#697). Purging by prefix would have taken them too.
    localStorage.setItem('croniq_theme', 'dark')
    localStorage.setItem('croniq_sidebar', 'collapsed')

    const { forgetLegacyTokens } = await import('./session')
    forgetLegacyTokens()

    expect(localStorage.getItem('croniq_theme')).toBe('dark')
    expect(localStorage.getItem('croniq_sidebar')).toBe('collapsed')
  })

  it('does nothing and throws nothing when there is nothing to remove', async () => {
    const { forgetLegacyTokens } = await import('./session')

    expect(() => forgetLegacyTokens()).not.toThrow()
  })
})
