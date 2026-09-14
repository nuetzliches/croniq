// @vitest-environment happy-dom
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * The reactive half of "you are signed out, go to the sign-in page".
 *
 * It exists because a guard alone cannot notice a session dying while someone
 * sits on a page — that produces no navigation. But it decides *where it is*
 * by reading `router.currentRoute`, and before the first navigation finalises
 * that is `START_LOCATION`: path `/`, no name, `meta` an empty object.
 *
 * So during a cold load it cannot tell that the page being navigated *to* is
 * public. A bootstrap refresh answering `no_session` in that window redirected
 * to `/login`, superseding the pending navigation — and an invitation link
 * carries its token in the URL, so the token went with it (issue #665).
 *
 * `happy-dom` because the router uses `createWebHistory`, which needs a
 * `window` to exist.
 */

const { installAuthWatch, router } = await import('./index')
const { useAuthStore } = await import('~/stores/auth')

describe('installAuthWatch', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.restoreAllMocks()
  })

  it('does not redirect before the first navigation has resolved', async () => {
    // START_LOCATION: what `currentRoute` holds on a cold load, before the
    // target route — public or not — is known.
    const replace = vi.spyOn(router, 'replace').mockResolvedValue(undefined)
    installAuthWatch()

    const auth = useAuthStore()
    auth.clear()
    await Promise.resolve()

    expect(replace).not.toHaveBeenCalled()
  })

  it('does not redirect away from a public route', async () => {
    // Signed in first, so the guard lets the navigation finish; the point of
    // the test is what happens to a *public* route when the session then dies.
    useAuthStore().setToken('access-token')
    await router.replace('/password-reset/confirm?token=abc')
    const replace = vi.spyOn(router, 'replace').mockResolvedValue(undefined)
    installAuthWatch()

    useAuthStore().clear()
    await Promise.resolve()

    expect(replace).not.toHaveBeenCalled()
  })

  it('redirects from a protected route, carrying where to come back to', async () => {
    // The case the watch exists for: the session died in place.
    useAuthStore().setToken('access-token')
    await router.replace('/jobs')
    const replace = vi.spyOn(router, 'replace').mockResolvedValue(undefined)
    installAuthWatch()

    useAuthStore().clear()
    await Promise.resolve()

    expect(replace).toHaveBeenCalledWith({ name: 'login', query: { next: '/jobs' } })
  })

  it('stays put on a deliberate sign-out, which navigates itself', async () => {
    useAuthStore().setToken('access-token')
    await router.replace('/jobs')
    const replace = vi.spyOn(router, 'replace').mockResolvedValue(undefined)
    installAuthWatch()

    const auth = useAuthStore()
    auth.clear({ deliberate: true })
    await Promise.resolve()

    expect(replace).not.toHaveBeenCalled()
  })
})
