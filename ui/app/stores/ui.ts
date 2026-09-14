import { ref, watch } from 'vue'
import { defineStore } from 'pinia'

/**
 * Per-browser display preferences.
 *
 * The storage keys are the React tree's, verbatim, and stay that way now that
 * it is gone. Both dashboards were served from the same origin and so shared a
 * `localStorage` namespace — which means an operator who had collapsed the
 * sidebar kept it collapsed across the cutover instead of having their
 * preferences silently reset by an upgrade they did not ask for. Renaming the
 * keys now would spend that for nothing.
 *
 * Keeping the keys was only half of it, though. The *values* have two shapes,
 * and reading the old one as the new one is worse than not reading it at all
 * (issue #666): a stored `'auto'` fell through the theme cast and produced
 * `data-theme="auto"`, which matches no stylesheet, so every operator who had
 * asked to follow their OS got a light dashboard on the morning of the
 * upgrade. The readers below handle both shapes and write back the new one.
 */
const SIDEBAR_KEY = 'croniq_sidebar'
const THEME_KEY = 'croniq_theme'

export type ThemePref = 'light' | 'dark' | 'system'

function read(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    // Private mode, or storage disabled. A preference is not worth an
    // exception on boot.
    return null
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value)
  } catch {
    // Same.
  }
}

/**
 * The stored theme, in whichever shape it is stored.
 *
 * The React tree wrote `'auto'` where this one writes `'system'`, and wrote it
 * on every mount — so nearly every existing browser has the key set, most of
 * them to a value this tree does not recognise. Casting the string to
 * `ThemePref` type-checks and is a lie: `applyTheme('auto')` sets
 * `data-theme="auto"` and clears the `dark` class, which is a light dashboard
 * for exactly the people who chose to follow their OS.
 *
 * Anything unrecognised falls back to the default rather than reaching the DOM.
 */
function readTheme(): ThemePref {
  const stored = read(THEME_KEY)
  if (stored === 'light' || stored === 'dark' || stored === 'system') return stored
  // The React tree's spelling of the same intent.
  if (stored === 'auto') return 'system'
  return 'system'
}

/**
 * The stored sidebar state, in whichever shape it is stored.
 *
 * This tree writes the literal `'collapsed'` / `'expanded'`. The React tree
 * used zustand's `persist`, which writes JSON: `{"state":{"collapsed":true}}`.
 * A strict `=== 'collapsed'` is never true for that payload, so a collapsed
 * sidebar silently sprang open on the cutover — the very thing the comment
 * above says was worth keeping the key for.
 */
function readSidebarCollapsed(): boolean {
  const stored = read(SIDEBAR_KEY)
  if (stored === null) return false
  if (stored === 'collapsed') return true
  if (stored === 'expanded') return false
  try {
    const parsed: unknown = JSON.parse(stored)
    const state = (parsed as { state?: { collapsed?: unknown } } | null)?.state
    return state?.collapsed === true
  } catch {
    // Not ours and not zustand's. Treat it as unset.
    return false
  }
}

export const useUiStore = defineStore('ui', () => {
  const sidebarCollapsed = ref(readSidebarCollapsed())
  const theme = ref<ThemePref>(readTheme())

  // Write the new shape back on boot, so a browser carrying a React-tree value
  // stops carrying it. The watchers below only fire on change, and a
  // preference that already reads correctly would otherwise stay in the old
  // format indefinitely.
  write(SIDEBAR_KEY, sidebarCollapsed.value ? 'collapsed' : 'expanded')
  write(THEME_KEY, theme.value)

  watch(sidebarCollapsed, (collapsed) => {
    write(SIDEBAR_KEY, collapsed ? 'collapsed' : 'expanded')
  })

  watch(
    theme,
    (pref) => {
      write(THEME_KEY, pref)
      applyTheme(pref)
    },
    { immediate: true },
  )

  function toggleSidebar() {
    sidebarCollapsed.value = !sidebarCollapsed.value
  }

  /**
   * Re-assert the stored theme on `<html>`.
   *
   * For a screen that deliberately overrides the reader's choice while it is
   * open — the sign-in stage is dark whatever the setting — and has to hand it
   * back on the way out.
   *
   * It re-applies from the store rather than restoring a snapshot the caller
   * took, and that distinction is the whole reason this exists. Snapshotting
   * the DOM looked equivalent and was not: on a cold load of `/login` the
   * page's `onMounted` ran *before* this store's theme watcher, captured an
   * attribute that was not set yet, and "restored" the reader to nothing.
   */
  function reapplyTheme() {
    applyTheme(theme.value)
  }

  return { sidebarCollapsed, theme, toggleSidebar, reapplyTheme }
})

/**
 * Put the resolved theme on `<html>`.
 *
 * `data-theme` and the `dark` class both, because Nuxt UI keys off the class
 * while the React tree's stylesheets key off the attribute — and during the
 * parallel phase a browser may hold preferences written by either.
 */
function applyTheme(pref: ThemePref) {
  const resolved =
    pref === 'system'
      ? window.matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light'
      : pref
  const root = document.documentElement
  root.dataset.theme = resolved
  root.classList.toggle('dark', resolved === 'dark')
}
