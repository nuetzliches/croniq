import { describe, expect, it } from 'vitest'
import { GO_TO } from '~/composables/useGoToShortcuts'
import { NAV_SECTIONS } from './nav'

/**
 * The navigation and its `g` chords, which have to agree.
 *
 * The chords are listed in two places — the table and the shell's hint, now
 * derived from it — and the command palette builds its "Go to" section from
 * the table. A screen added to the sidebar without a chord, or a chord left
 * pointing at a screen that moved, is easy to miss by hand.
 */
describe('the navigation', () => {
  const items = NAV_SECTIONS.flatMap((section) => section.items)

  it('puts Notes directly under Runs, with its badge', () => {
    const operations = NAV_SECTIONS.find((section) => section.label === 'Operations')!.items.map(
      (item) => item.to,
    )
    expect(operations.indexOf('/notes')).toBe(operations.indexOf('/executions') + 1)
    expect(items.find((item) => item.to === '/notes')?.badge).toBe('notes')
  })

  it('gives every chord a key of its own, and leaves t and g alone', () => {
    // `t` switches the time display window-wide and `g` arms the chord: a
    // chord on either would fire both.
    const keys = GO_TO.map((entry) => entry.key)
    expect(new Set(keys).size).toBe(keys.length)
    expect(keys).not.toContain('t')
    expect(keys).not.toContain('g')
  })

  it('has a chord for every screen in the navigation, and none for anything else', () => {
    expect(GO_TO.map((entry) => entry.path).sort()).toEqual(items.map((item) => item.to).sort())
  })
})
