import { ref } from 'vue'
import type { VersionResponse } from '~/api/types'

/**
 * "The server was upgraded underneath this tab."
 *
 * A single-page app loads its bundle once. Restarting `croniq-server` on a new
 * release does not reload anything in an open tab: navigation is client-side,
 * every request still succeeds, and the operator keeps working in the old
 * dashboard — with the old version in the header — across any number of page
 * changes and button clicks. Until this, `/version` was even pinned with
 * `staleTime: Infinity` on the belief that a restart reloads the page; it does
 * not, so the chip stayed on the version the tab was opened with.
 *
 * This is a different condition from the skew `VersionSkewBanner` reports. That
 * one is a pair pinned apart by hand and survives a reload; this one is cured by
 * a reload, and a reload is what it asks for.
 *
 * The comparison is against the first `/version` answer this page load saw, not
 * against the bundle's stamped `UI_VERSION`: an unstamped build (every build
 * from source) has no version to compare, and it goes stale just the same.
 */

export interface ServerUpdate {
  /** The server version this page load started against. */
  from: string
  /** The server version answering now. */
  to: string
}

/** Placeholder values a build reports when it was not stamped. */
const UNSTAMPED = new Set(['', 'dev', 'unknown'])

function clean(value: string | null | undefined): string | null {
  if (typeof value !== 'string') return null
  const trimmed = value.trim()
  if (UNSTAMPED.has(trimmed)) return null
  return trimmed.startsWith('v') ? trimmed.slice(1) : trimmed
}

/**
 * Tracks one page load's baseline. A factory so tests get a fresh one; the app
 * uses the module-level instance below.
 */
export function createServerUpdateTracker() {
  let baseline: { version: string; sha: string | null } | null = null
  const update = ref<ServerUpdate | null>(null)

  function observe(answer: VersionResponse | null | undefined) {
    const version = clean(answer?.version)
    if (!version) return
    const sha = clean(answer?.git_sha)
    if (!baseline) {
      baseline = { version, sha }
      return
    }
    // The sha as well as the version: a rebuild under the same version number
    // (a re-cut tag, a source build redeployed) ships a different bundle too.
    // Only when both sides have one — an unstamped sha is no evidence.
    const changed =
      version !== baseline.version || (sha !== null && baseline.sha !== null && sha !== baseline.sha)
    // Sticky once seen. During a rolling deploy two replicas may answer in
    // turn; the old bundle is stale the moment a new one exists, and a banner
    // that flickers with the load balancer is worse than one that stays.
    if (changed && !update.value) update.value = { from: baseline.version, to: version }
  }

  return { update, observe }
}

const tracker = createServerUpdateTracker()

/** Set once the server this tab talks to is not the one it was loaded from. */
export const serverUpdate = tracker.update
export const observeServerVersion = tracker.observe

/**
 * A lazy route chunk that would not load.
 *
 * After an upgrade the old bundle's chunk names no longer exist on the server
 * (they are content-hashed), so the first navigation to a screen this tab had
 * not opened yet fails before the `/version` poll has noticed anything. The
 * messages are the browsers' own; there is no error class to test for.
 */
export function isChunkLoadError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error ?? '')
  return (
    /Failed to fetch dynamically imported module/i.test(message) || // Chromium
    /error loading dynamically imported module/i.test(message) || // Firefox
    /Importing a module script failed/i.test(message) || // Safari
    /Unable to preload CSS/i.test(message) // Vite's own preload helper
  )
}

const RELOAD_KEY = 'croniq_stale_bundle_reload'
/** Below this, a second automatic reload is taken to be a loop and refused. */
const RELOAD_COOLDOWN_MS = 10_000

/**
 * Load `href` as a full page, so it comes with the server's current bundle.
 *
 * Refuses if it already did so within the cooldown. A chunk that is missing for
 * some other reason — a broken deploy, a proxy serving stale `index.html` —
 * would otherwise reload forever; refusing leaves the page up with whatever
 * error the navigation produced, which is the honest outcome.
 *
 * Returns whether it navigated.
 */
export function reloadInto(href: string, now: number = Date.now()): boolean {
  try {
    const last = Number(sessionStorage.getItem(RELOAD_KEY))
    if (last && now - last < RELOAD_COOLDOWN_MS) return false
    sessionStorage.setItem(RELOAD_KEY, String(now))
  } catch {
    // Storage disabled: no loop guard, but a stale tab is the more likely
    // problem than a reload loop, so still reload.
  }
  window.location.assign(href)
  return true
}
