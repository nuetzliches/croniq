// @vitest-environment happy-dom
import { createPinia, setActivePinia } from 'pinia'
import { effectScope } from 'vue'
import { beforeEach, describe, expect, it } from 'vitest'
import { useUiStore } from '~/stores/ui'
import { useTimeDisplay, useTimeDisplayShortcut } from './useTimeDisplay'

function press(init: KeyboardEventInit, target: EventTarget = window) {
  target.dispatchEvent(new KeyboardEvent('keydown', { key: 't', bubbles: true, ...init }))
}

describe('useTimeDisplay', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
  })

  it('reads relative until switched, then the clock', () => {
    const iso = new Date(Date.now() - 3 * 60_000).toISOString()
    const times = useTimeDisplay()

    expect(times.text(iso)).toBe('3 min ago')

    times.toggle()

    expect(times.clock.value).toBe(true)
    expect(times.text(iso)).toMatch(/^(\S+ )?\d{2}:\d{2}:\d{2}$/)
    // The tooltip carries the other form.
    expect(times.title(iso)).toMatch(/^3 min ago · /)
  })

  it('has no tooltip for no instant', () => {
    expect(useTimeDisplay().title(null)).toBe('')
  })
})

/**
 * A screen can mount several headings — two on Jobs, one per table on a page
 * with more than one. The key has to toggle once however many there are, or
 * an even number of headings would make `t` do nothing at all.
 */
describe('useTimeDisplayShortcut', () => {
  beforeEach(() => {
    localStorage.clear()
    setActivePinia(createPinia())
  })

  it('toggles once per press, however many headings ask for it', () => {
    const scope = effectScope()
    scope.run(() => {
      useTimeDisplayShortcut()
      useTimeDisplayShortcut()
    })
    const ui = useUiStore()

    press({})
    expect(ui.timeDisplay).toBe('absolute')

    scope.stop()
  })

  it('leaves the key alone once the last heading is gone', () => {
    const first = effectScope()
    const second = effectScope()
    first.run(useTimeDisplayShortcut)
    second.run(useTimeDisplayShortcut)
    const ui = useUiStore()

    first.stop()
    press({})
    expect(ui.timeDisplay).toBe('absolute')

    second.stop()
    press({})
    expect(ui.timeDisplay).toBe('absolute')
  })

  it("ignores Ctrl+T and the like, and a held key's repeats", () => {
    const scope = effectScope()
    scope.run(useTimeDisplayShortcut)
    const ui = useUiStore()

    press({ ctrlKey: true })
    press({ metaKey: true })
    press({ altKey: true })
    press({ repeat: true })
    expect(ui.timeDisplay).toBe('relative')

    scope.stop()
  })

  it('never takes a `t` typed into a field', () => {
    const scope = effectScope()
    scope.run(useTimeDisplayShortcut)
    const input = document.createElement('input')
    document.body.append(input)

    press({}, input)
    expect(useUiStore().timeDisplay).toBe('relative')

    input.remove()
    scope.stop()
  })
})
