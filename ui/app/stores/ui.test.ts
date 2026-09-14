// @vitest-environment happy-dom
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import { useUiStore } from './ui'

/**
 * Preferences written by the React dashboard, read by this one.
 *
 * Both were served from the same origin, so they share a `localStorage`
 * namespace, and the store keeps the old keys deliberately — an operator's
 * choices should survive an upgrade they did not ask for. Keeping the keys was
 * only half of it: the *values* have two shapes, and misreading the old one is
 * worse than ignoring it (issue #666).
 *
 * A stored `'auto'` fell through the theme cast and produced
 * `data-theme="auto"`, which matches no stylesheet. Every operator who had
 * asked to follow their OS got a light dashboard on the morning of the
 * upgrade — and the release notes said their preferences would carry over.
 */
describe('ui store, reading what the React tree left behind', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
    document.documentElement.removeAttribute('data-theme')
    document.documentElement.classList.remove('dark')
  })

  it("reads the React tree's 'auto' as 'system'", () => {
    localStorage.setItem('croniq_theme', 'auto')

    const ui = useUiStore()

    expect(ui.theme).toBe('system')
    // The symptom, asserted directly: an unresolved value on the element is
    // what produced the light dashboard.
    expect(document.documentElement.dataset.theme).toMatch(/^(light|dark)$/)
  })

  it('rewrites the old value so the browser stops carrying it', () => {
    localStorage.setItem('croniq_theme', 'auto')

    useUiStore()

    expect(localStorage.getItem('croniq_theme')).toBe('system')
  })

  it('keeps an explicit choice, in either spelling', () => {
    localStorage.setItem('croniq_theme', 'dark')

    const ui = useUiStore()

    expect(ui.theme).toBe('dark')
    expect(document.documentElement.dataset.theme).toBe('dark')
  })

  it('falls back to the default for a value from neither tree', () => {
    localStorage.setItem('croniq_theme', 'solarized')

    expect(useUiStore().theme).toBe('system')
  })

  it("reads zustand's persisted sidebar payload", () => {
    // What `persist({ name: 'croniq_sidebar' })` actually writes.
    localStorage.setItem('croniq_sidebar', JSON.stringify({ state: { collapsed: true }, version: 0 }))

    expect(useUiStore().sidebarCollapsed).toBe(true)
  })

  it('reads its own format too', () => {
    localStorage.setItem('croniq_sidebar', 'collapsed')

    expect(useUiStore().sidebarCollapsed).toBe(true)
  })

  it('rewrites the zustand payload into its own format', () => {
    localStorage.setItem('croniq_sidebar', JSON.stringify({ state: { collapsed: true } }))

    useUiStore()

    expect(localStorage.getItem('croniq_sidebar')).toBe('collapsed')
  })

  it('treats an unparseable sidebar value as unset rather than throwing', () => {
    localStorage.setItem('croniq_sidebar', '{not json')

    expect(useUiStore().sidebarCollapsed).toBe(false)
  })

  it('starts from the defaults when nothing is stored', () => {
    const ui = useUiStore()

    expect(ui.theme).toBe('system')
    expect(ui.sidebarCollapsed).toBe(false)
  })
})
