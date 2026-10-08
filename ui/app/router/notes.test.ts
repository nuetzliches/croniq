// @vitest-environment happy-dom
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import { useAuthStore } from '~/stores/auth'
import { router } from './index'

/**
 * The inbox's three routes are one screen.
 *
 * The list and both kinds of thread render the same component, as Runs does
 * with its detail — and here that carries more than the list's scroll: the
 * component instance is the visit, and the visit is what keeps "new" marked as
 * new while one thread after another is opened (useNotesInbox). A second
 * component would start a new visit on every click, and the second thread
 * opened would find nothing new any more.
 */
describe('the notes inbox routes', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    useAuthStore().setToken('access-token')
  })

  it('addresses a run thread by its run and a job thread by its job', async () => {
    await router.push('/notes/runs/6f9619ff-8b86-d011-b42d-00c04fc964ff')
    expect(router.currentRoute.value.name).toBe('notes-run')
    expect(router.currentRoute.value.params.id).toBe('6f9619ff-8b86-d011-b42d-00c04fc964ff')

    await router.push('/notes/jobs/demo%3Aheartbeat')
    expect(router.currentRoute.value.name).toBe('notes-job')
    expect(router.currentRoute.value.params.jobKey).toBe('demo:heartbeat')
  })

  it('renders the list and both kinds of thread with one component', async () => {
    const rendered = new Set<unknown>()
    for (const path of ['/notes', '/notes/runs/run-1', '/notes/jobs/mail%3Asend']) {
      await router.push(path)
      // Once navigated, the router holds the loaded component in place of
      // the lazy loader, so this compares components, not three closures.
      rendered.add(router.currentRoute.value.matched.at(-1)?.components?.default)
    }
    expect(rendered.size).toBe(1)
  })
})
