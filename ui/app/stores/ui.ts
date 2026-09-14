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

export const useUiStore = defineStore('ui', () => {
  const sidebarCollapsed = ref(read(SIDEBAR_KEY) === 'collapsed')
  const theme = ref<ThemePref>((read(THEME_KEY) as ThemePref | null) ?? 'system')

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
