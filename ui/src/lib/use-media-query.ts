import { useEffect, useState } from 'react'

/// Subscribe to a CSS media query and return its current match state.
/// Returns `false` during SSR / first paint and updates on resize.
export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return false
    return window.matchMedia(query).matches
  })

  useEffect(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return
    const mql = window.matchMedia(query)
    const onChange = (e: MediaQueryListEvent) => setMatches(e.matches)
    // Sync once in case the lazy initial state was computed for a
    // different query (e.g. when this hook is called after a hot
    // reload swaps the breakpoint string).
    if (mql.matches !== matches) setMatches(mql.matches)
    mql.addEventListener('change', onChange)
    return () => mql.removeEventListener('change', onChange)
    // `matches` intentionally omitted — including it would re-subscribe
    // every time the value flips, which is wasteful and (in worst
    // cases) loops via `setMatches`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query])

  return matches
}
