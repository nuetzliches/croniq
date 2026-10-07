// @vitest-environment happy-dom
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import { DETAIL_PANE_DEFAULT_WIDTH, DETAIL_PANE_WIDE_DEFAULT_WIDTH, useUiStore } from './ui'

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

describe('ui store, how tables show a time', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
  })

  it('shows relative times until asked otherwise', () => {
    expect(useUiStore().timeDisplay).toBe('relative')
  })

  it('toggles, and remembers the choice for the next visit', async () => {
    const ui = useUiStore()

    ui.toggleTimeDisplay()
    await nextTick()

    expect(ui.timeDisplay).toBe('absolute')
    expect(localStorage.getItem('croniq_time_display')).toBe('absolute')

    setActivePinia(createPinia())
    expect(useUiStore().timeDisplay).toBe('absolute')
  })

  it('reads an unknown stored value as the default', () => {
    localStorage.setItem('croniq_time_display', 'sundial')

    expect(useUiStore().timeDisplay).toBe('relative')
  })
})

describe('ui store, the detail pane widths', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
  })

  it('starts at the default until someone drags it', () => {
    expect(useUiStore().runDetailWidth).toBe(DETAIL_PANE_DEFAULT_WIDTH)
  })

  it('remembers a dragged width for the next visit', async () => {
    const ui = useUiStore()

    ui.runDetailWidth = 640.4
    await nextTick()

    expect(localStorage.getItem('croniq_run_detail_width')).toBe('640')

    setActivePinia(createPinia())
    expect(useUiStore().runDetailWidth).toBe(640)
  })

  it.each(['wide', '', '-5', '100'])('reads %j as the default', (stored) => {
    localStorage.setItem('croniq_run_detail_width', stored)

    expect(useUiStore().runDetailWidth).toBe(DETAIL_PANE_DEFAULT_WIDTH)
  })

  it('keeps the dead letter detail apart from the run detail', async () => {
    localStorage.setItem('croniq_dead_letter_detail_width', '520')
    const ui = useUiStore()

    expect(ui.deadLetterDetailWidth).toBe(520)
    expect(ui.runDetailWidth).toBe(DETAIL_PANE_DEFAULT_WIDTH)

    ui.runDetailWidth = 700
    await nextTick()

    expect(localStorage.getItem('croniq_dead_letter_detail_width')).toBe('520')
    expect(localStorage.getItem('croniq_run_detail_width')).toBe('700')
  })

  it('starts the job, calendar and alert rule details at the wider default', async () => {
    const ui = useUiStore()
    expect(ui.jobDetailWidth).toBe(DETAIL_PANE_WIDE_DEFAULT_WIDTH)
    expect(ui.calendarDetailWidth).toBe(DETAIL_PANE_WIDE_DEFAULT_WIDTH)
    expect(ui.alertRuleDetailWidth).toBe(DETAIL_PANE_WIDE_DEFAULT_WIDTH)

    localStorage.setItem('croniq_job_detail_width', 'wide')
    setActivePinia(createPinia())
    expect(useUiStore().jobDetailWidth).toBe(DETAIL_PANE_WIDE_DEFAULT_WIDTH)

    const next = useUiStore()
    next.jobDetailWidth = 600
    await nextTick()
    expect(localStorage.getItem('croniq_job_detail_width')).toBe('600')
    expect(localStorage.getItem('croniq_calendar_detail_width')).toBe(null)

    next.calendarDetailWidth = 540
    await nextTick()
    expect(localStorage.getItem('croniq_calendar_detail_width')).toBe('540')

    next.alertRuleDetailWidth = 500
    await nextTick()
    expect(localStorage.getItem('croniq_alert_rule_detail_width')).toBe('500')
  })

  it('keeps a stored width wider than any window, for the view to clamp', () => {
    localStorage.setItem('croniq_run_detail_width', '5000')

    expect(useUiStore().runDetailWidth).toBe(5000)
  })
})
