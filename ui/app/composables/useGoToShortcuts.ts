import { onScopeDispose, ref } from 'vue'
import { useRouter } from 'vue-router'

/**
 * `g` then a letter, to go somewhere.
 *
 * The React command palette listed these — `G D`, `G J`, `G E` — beside its
 * entries as though they worked. Nothing implemented them: pressing `g` then
 * `d` did nothing at all, on any screen. Rather than carry a hint that lies,
 * the chords are real here, and the palette shows them because they are.
 *
 * A two-key chord rather than a modifier: this is an operations console, the
 * runs list already answers `j`/`k`, and every plain-letter modifier
 * combination worth having is already claimed by the browser. The convention
 * is borrowed from the places that made it familiar.
 */
const CHORD_WINDOW_MS = 1200

export const GO_TO: { key: string; label: string; path: string }[] = [
  { key: 'd', label: 'Dashboard', path: '/' },
  { key: 'r', label: 'Runs', path: '/executions' },
  { key: 'n', label: 'Runners', path: '/runners' },
  { key: 'x', label: 'Dead Letters', path: '/dead-letters' },
  { key: 'j', label: 'Jobs', path: '/jobs' },
  { key: 'c', label: 'Calendars', path: '/calendars' },
  { key: 'a', label: 'Alerts', path: '/alerts' },
  { key: 'l', label: 'Console', path: '/console' },
  { key: 's', label: 'Settings', path: '/settings' },
]

/** True while a field has focus — never steal a key someone is typing. */
function typing(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null
  if (!element) return false
  if (element.isContentEditable) return true
  return ['INPUT', 'TEXTAREA', 'SELECT'].includes(element.tagName)
}

export function useGoToShortcuts() {
  const router = useRouter()
  /** Whether `g` is armed — exposed so the shell can show that it is. */
  const armed = ref(false)
  let timer: ReturnType<typeof setTimeout> | undefined

  function disarm() {
    armed.value = false
    clearTimeout(timer)
  }

  function onKeydown(event: KeyboardEvent) {
    if (typing(event.target)) return
    if (event.metaKey || event.ctrlKey || event.altKey) return

    if (armed.value) {
      const hit = GO_TO.find((entry) => entry.key === event.key.toLowerCase())
      disarm()
      if (hit) {
        event.preventDefault()
        void router.push(hit.path)
      }
      return
    }

    if (event.key.toLowerCase() === 'g') {
      armed.value = true
      // The chord expires. Without this, a `g` typed and abandoned turns the
      // next unrelated keystroke into a navigation — minutes later.
      timer = setTimeout(disarm, CHORD_WINDOW_MS)
    }
  }

  window.addEventListener('keydown', onKeydown)
  onScopeDispose(() => {
    window.removeEventListener('keydown', onKeydown)
    clearTimeout(timer)
  })

  return { armed }
}
