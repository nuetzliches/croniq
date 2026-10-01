// @vitest-environment happy-dom
import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { router } from '~/router'
import { useAuthStore } from '~/stores/auth'
import {
  createServerUpdateTracker,
  isChunkLoadError,
  reloadInto,
  serverUpdate,
} from './server-update'

const answer = (version: string, git_sha = 'abc1234') => ({
  version,
  git_sha,
  build_time: '2026-10-01T00:00:00Z',
  env: 'production',
})

/**
 * An open tab kept running — and showing — v0.40.0 against a server already
 * upgraded to v0.42.0, across page changes and action clicks, until someone
 * pressed F5. The tracker is what notices; the router guard is what acts.
 */
describe('server update tracker', () => {
  it('takes the first answer as the baseline and reports nothing', () => {
    const t = createServerUpdateTracker()
    t.observe(answer('0.40.0'))
    t.observe(answer('0.40.0'))
    expect(t.update.value).toBeNull()
  })

  it('reports an upgrade underneath the page', () => {
    const t = createServerUpdateTracker()
    t.observe(answer('0.40.0'))
    t.observe(answer('0.42.0', 'def5678'))
    expect(t.update.value).toEqual({ from: '0.40.0', to: '0.42.0' })
  })

  it('reports a rebuild under the same version', () => {
    const t = createServerUpdateTracker()
    t.observe(answer('0.42.0', 'abc1234'))
    t.observe(answer('0.42.0', 'def5678'))
    expect(t.update.value).toEqual({ from: '0.42.0', to: '0.42.0' })
  })

  it('does not take an unstamped sha as evidence', () => {
    const t = createServerUpdateTracker()
    t.observe(answer('0.42.0', 'unknown'))
    t.observe(answer('0.42.0', 'def5678'))
    expect(t.update.value).toBeNull()
  })

  it('does not mistake a v-prefix for an upgrade', () => {
    const t = createServerUpdateTracker()
    t.observe(answer('0.42.0'))
    t.observe(answer('v0.42.0'))
    expect(t.update.value).toBeNull()
  })

  it('ignores missing and unstamped answers, before and after the baseline', () => {
    const t = createServerUpdateTracker()
    t.observe(undefined)
    t.observe(answer('dev'))
    t.observe(answer('0.40.0'))
    t.observe(undefined)
    t.observe(answer(''))
    expect(t.update.value).toBeNull()
  })

  it('stays reported when an old replica answers again', () => {
    // A rolling deploy alternates replicas. The old bundle is stale the moment
    // a new release exists; a banner flickering with the load balancer is not
    // something to read.
    const t = createServerUpdateTracker()
    t.observe(answer('0.40.0'))
    t.observe(answer('0.42.0', 'def5678'))
    t.observe(answer('0.40.0'))
    expect(t.update.value).toEqual({ from: '0.40.0', to: '0.42.0' })
  })
})

describe('isChunkLoadError', () => {
  it.each([
    'Failed to fetch dynamically imported module: https://x/assets/JobsView-abc.js',
    'error loading dynamically imported module: https://x/assets/JobsView-abc.js',
    'Importing a module script failed.',
    'Unable to preload CSS for /assets/JobsView-abc.css',
  ])('recognises %o', (message) => {
    expect(isChunkLoadError(new TypeError(message))).toBe(true)
  })

  it('leaves other errors alone', () => {
    expect(isChunkLoadError(new Error('Request failed with status 500'))).toBe(false)
    expect(isChunkLoadError(undefined)).toBe(false)
  })
})

describe('reloadInto', () => {
  let assign: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    sessionStorage.clear()
    assign = vi.spyOn(window.location, 'assign').mockImplementation(() => undefined)
  })

  afterEach(() => {
    assign.mockRestore()
  })

  it('navigates as a full page load', () => {
    expect(reloadInto('/jobs', 1_000_000)).toBe(true)
    expect(assign).toHaveBeenCalledWith('/jobs')
  })

  it('refuses a second reload within the cooldown, so a broken deploy cannot loop', () => {
    reloadInto('/jobs', 1_000_000)
    expect(reloadInto('/jobs', 1_005_000)).toBe(false)
    expect(assign).toHaveBeenCalledTimes(1)
  })

  it('reloads again once the cooldown has passed', () => {
    reloadInto('/jobs', 1_000_000)
    expect(reloadInto('/runners', 1_020_000)).toBe(true)
    expect(assign).toHaveBeenLastCalledWith('/runners')
  })
})

describe('router, after the server was upgraded', () => {
  let assign: ReturnType<typeof vi.spyOn>

  beforeEach(async () => {
    sessionStorage.clear()
    setActivePinia(createPinia())
    useAuthStore().setToken('access-token')
    assign = vi.spyOn(window.location, 'assign').mockImplementation(() => undefined)
    serverUpdate.value = null
    await router.push('/runners')
  })

  afterEach(() => {
    serverUpdate.value = null
    assign.mockRestore()
  })

  it('navigates in place while nothing changed', async () => {
    await router.push('/jobs')
    expect(router.currentRoute.value.name).toBe('jobs')
    expect(assign).not.toHaveBeenCalled()
  })

  it('turns the next page change into a full load of the target', async () => {
    serverUpdate.value = { from: '0.40.0', to: '0.42.0' }

    await router.push('/jobs?status=failed')

    expect(assign).toHaveBeenCalledWith('/jobs?status=failed')
    // The old bundle does not render the screen; the reloaded page will.
    expect(router.currentRoute.value.name).toBe('runners')
  })

  it('still navigates in place if the reload was refused', async () => {
    serverUpdate.value = { from: '0.40.0', to: '0.42.0' }
    reloadInto('/elsewhere') // inside the cooldown now
    assign.mockClear()

    await router.push('/jobs')

    expect(assign).not.toHaveBeenCalled()
    expect(router.currentRoute.value.name).toBe('jobs')
  })
})
