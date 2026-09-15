import { effectScope, ref } from 'vue'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useDebounced } from './useDebounced'

/**
 * Typing eight characters used to be eight `limit=200` requests, seven of them
 * answered and discarded (issue #730).
 *
 * The tests run inside an `effectScope` because the composable registers an
 * `onScopeDispose` cleanup, and because disposing is half of what is asserted.
 */
describe('useDebounced', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  /** Vue watchers are scheduled, so a flush has to happen before the timer. */
  async function settle() {
    await Promise.resolve()
  }

  it('holds the value until typing stops', async () => {
    const scope = effectScope()
    const typed = ref('')
    const debounced = scope.run(() => useDebounced(() => typed.value))!

    for (const char of 'nightly-') {
      typed.value += char
      await settle()
      vi.advanceTimersByTime(40)
    }

    // Mid-word: eight keystrokes, and the query has not moved once.
    expect(debounced.value).toBe('')

    vi.advanceTimersByTime(250)
    expect(debounced.value).toBe('nightly-')
    scope.stop()
  })

  it('waits from the last keystroke, not the first', async () => {
    const scope = effectScope()
    const typed = ref('')
    const debounced = scope.run(() => useDebounced(() => typed.value))!

    typed.value = 'a'
    await settle()
    vi.advanceTimersByTime(240)
    typed.value = 'ab'
    await settle()
    vi.advanceTimersByTime(240)

    // 480ms in, and still nothing — the first timer was superseded.
    expect(debounced.value).toBe('')

    vi.advanceTimersByTime(10)
    expect(debounced.value).toBe('ab')
    scope.stop()
  })

  it('clears at once', async () => {
    // Emptying the box is deliberate and its result is obvious, so waiting
    // reads as a control that did not work.
    const scope = effectScope()
    const typed = ref('nightly')
    const debounced = scope.run(() => useDebounced(() => typed.value))!

    vi.advanceTimersByTime(250)
    typed.value = ''
    await settle()

    expect(debounced.value).toBe('')
    scope.stop()
  })

  it('starts on whatever the source already says', () => {
    // A pasted link arrives with its filter in the URL; the first request has
    // to carry it rather than fetch everything and narrow a beat later.
    const scope = effectScope()
    const debounced = scope.run(() => useDebounced(() => 'from-the-url'))!

    expect(debounced.value).toBe('from-the-url')
    scope.stop()
  })

  it('drops a pending update when the screen goes away', async () => {
    const scope = effectScope()
    const typed = ref('')
    const debounced = scope.run(() => useDebounced(() => typed.value))!

    typed.value = 'half-typed'
    await settle()
    scope.stop()
    vi.advanceTimersByTime(1000)

    expect(debounced.value).toBe('')
  })

  it('takes a custom delay', async () => {
    const scope = effectScope()
    const typed = ref('')
    const debounced = scope.run(() => useDebounced(() => typed.value, { delayMs: 800 }))!

    typed.value = 'slow'
    await settle()
    vi.advanceTimersByTime(700)
    expect(debounced.value).toBe('')

    vi.advanceTimersByTime(100)
    expect(debounced.value).toBe('slow')
    scope.stop()
  })
})
