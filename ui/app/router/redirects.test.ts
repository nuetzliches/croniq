// @vitest-environment happy-dom
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import { useAuthStore } from '~/stores/auth'
import { router } from './index'

/**
 * URLs the React dashboard handed out and this one has to keep honouring.
 *
 * `/runners/:runnerId` and `/dead-letters/:id` were real pages until v0.38.0,
 * and the app emitted links to them from its entity links and its dashboard —
 * so they are in bookmarks, in alert bodies and in chat history. Without a
 * redirect they land on the not-found page, which reads as "that runner is
 * gone" rather than "that page moved" (issue #669).
 *
 * These navigate rather than `resolve`: a redirect is only followed during a
 * navigation, so resolving one just returns the redirect record itself.
 */
describe('routes that outlived their pages', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    // Signed in, so the auth guard lets the navigation through to the route
    // table, which is what these are about.
    useAuthStore().setToken('access-token')
  })

  it('sends an old runner link to the fleet', async () => {
    await router.push('/runners/runner-7')

    expect(router.currentRoute.value.name).toBe('runners')
  })

  it('sends an old dead-letter link to the queue, carrying the row', async () => {
    // The id survives as `?selected=`, which the view reads — so the link
    // still opens the thing it named, not just the screen it lived on.
    await router.push('/dead-letters/6f9619ff-8b86-d011-b42d-00c04fc964ff')

    expect(router.currentRoute.value.name).toBe('dead-letters')
    expect(router.currentRoute.value.query.selected).toBe(
      '6f9619ff-8b86-d011-b42d-00c04fc964ff',
    )
  })

  it('does not swallow the list routes themselves', async () => {
    await router.push('/runners')
    expect(router.currentRoute.value.name).toBe('runners')

    await router.push('/dead-letters')
    expect(router.currentRoute.value.name).toBe('dead-letters')
    expect(router.currentRoute.value.query.selected).toBeUndefined()
  })

  it('no longer serves the scaffold check', async () => {
    // It said so itself: "deleted when the shell lands in step 2" (issue #672).
    // The shell landed. A verification page inside the authenticated app is a
    // route an operator can reach and cannot interpret.
    await router.push('/scaffold')

    expect(router.currentRoute.value.name).toBe('not-found')
  })
})
