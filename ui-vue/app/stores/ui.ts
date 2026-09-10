import { ref, watch } from 'vue'
import { defineStore } from 'pinia'

/**
 * Per-browser display preferences.
 *
 * The storage keys are the React tree's, verbatim. Both dashboards are served
 * from the same origin, so they share a `localStorage` namespace — which means
 * an operator who has collapsed the sidebar keeps it collapsed across the
 * cutover instead of having their preferences silently reset by an upgrade
 * they did not ask for.
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

  return { sidebarCollapsed, theme, toggleSidebar }
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
