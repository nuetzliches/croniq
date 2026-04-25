import { useEffect, useState } from 'react'

type ThemePref = 'light' | 'dark' | 'auto'
type ResolvedTheme = 'light' | 'dark'

const STORAGE_KEY = 'croniq_theme'

function readPref(): ThemePref {
  const stored = localStorage.getItem(STORAGE_KEY)
  if (stored === 'light' || stored === 'dark' || stored === 'auto') return stored
  return 'auto'
}

function systemTheme(): ResolvedTheme {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function resolve(pref: ThemePref): ResolvedTheme {
  return pref === 'auto' ? systemTheme() : pref
}

function applyResolved(resolved: ResolvedTheme) {
  document.documentElement.classList.toggle('dark', resolved === 'dark')
}

function persist(pref: ThemePref) {
  localStorage.setItem(STORAGE_KEY, pref)
}

export function useTheme() {
  const [pref, setPref] = useState<ThemePref>(readPref)
  const [resolved, setResolved] = useState<ResolvedTheme>(() => resolve(readPref()))

  useEffect(() => {
    persist(pref)
    setResolved(resolve(pref))
  }, [pref])

  // Track OS-level changes only while the user is on `auto`. Switching
  // to an explicit pref unsubscribes; switching back resubscribes.
  useEffect(() => {
    if (pref !== 'auto') return
    const mql = window.matchMedia('(prefers-color-scheme: dark)')
    const handler = () => setResolved(systemTheme())
    mql.addEventListener('change', handler)
    return () => mql.removeEventListener('change', handler)
  }, [pref])

  useEffect(() => {
    applyResolved(resolved)
  }, [resolved])

  // Cycle: light → dark → auto → light. Three clicks gets you back to
  // where you started — gives the user the "follow OS" option without
  // burying it in a menu.
  function toggle() {
    setPref((p) => (p === 'light' ? 'dark' : p === 'dark' ? 'auto' : 'light'))
  }

  return { theme: resolved, pref, toggle }
}

// Apply theme immediately on module load to avoid a flash of the wrong
// theme during hydration.
applyResolved(resolve(readPref()))
