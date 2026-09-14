/**
 * What this bundle is, and whether it matches the server it is talking to.
 *
 * The combined `croniq` image cannot skew: one digest, both halves. The split
 * images published in #587/#598 can — `croniq-ui:0.39.0` against
 * `croniq-server:0.38.0` gives a dashboard calling endpoints the server may not
 * have, and the symptom is a scattering of 404s on individual screens rather
 * than anything that names the cause. The release workflow publishes all three
 * variants under the same tags from the same run and `docs/operations.md`
 * states the lockstep rule, but a documented convention is not a guard:
 * nothing stops an operator pinning them apart, and nothing tells them they
 * did. This is issue #604.
 *
 * The version is stamped at build time from a Docker `ARG` (see the
 * `ui-builder` stage). A build that was not stamped — every `npm run build` on
 * a laptop, and every CI build that is not a release — reports nothing, and
 * "nothing" is treated as *do not warn*. A dev build nagging about skew on
 * every reload is how a warning gets ignored on the one day it is real.
 */
const RAW = import.meta.env.VITE_APP_VERSION

/**
 * This bundle's version, or `null` when it was not stamped.
 *
 * `'dev'` is spelled out as a value rather than left to an empty string: the
 * Dockerfile's `ARG` defaults to it, so a local `docker build` with no
 * `--build-arg` produces an unstamped bundle rather than one claiming to be
 * whatever version the file happened to say.
 */
export const UI_VERSION: string | null = normalise(RAW)

function normalise(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const trimmed = value.trim()
  if (!trimmed || trimmed === 'dev' || trimmed === 'unknown') return null
  // Tags are `v0.38.0`; `CARGO_PKG_VERSION` is `0.38.0`. Comparing those two
  // as strings would report a permanent skew on a correctly matched pair,
  // which is the worst possible failure for a warning of this kind — it would
  // be wrong every time until someone switched it off.
  return trimmed.startsWith('v') ? trimmed.slice(1) : trimmed
}

export interface Skew {
  ui: string
  server: string
}

/**
 * `null` when there is nothing to report, otherwise the two versions.
 *
 * Compared exactly rather than by major/minor. The release publishes the three
 * images under the same tag from one run, so *any* difference means they were
 * pinned apart by hand — and a patch-level difference is still a pair nobody
 * tested together. Loosening this to "same minor is fine" would invent a
 * compatibility promise the project has not made.
 */
export function versionSkew(
  uiVersion: string | null | undefined,
  serverVersion: string | null | undefined,
): Skew | null {
  // Both sides, not just the server's. `UI_VERSION` arrives normalised
  // already, so normalising it again is a no-op in production — but a function
  // that silently trusts one argument and cleans the other is a trap for the
  // next caller, and the tests caught exactly that: an un-normalised `'dev'`
  // was reported as a skew against a real server version.
  const ui = normalise(uiVersion)
  const server = normalise(serverVersion)
  if (!ui || !server) return null
  return ui === server ? null : { ui, server }
}

/**
 * The dismissal key for one specific pair.
 *
 * Keyed by the pair, not by a flag, so dismissing "0.39.0 against 0.38.0"
 * hides that and only that. Upgrade the server into a *different* mismatch and
 * the warning comes back, which is the whole point — a dismissal that silences
 * every future skew is a dismissal that silences the next real one.
 */
export function skewDismissalKey(skew: Skew): string {
  return `croniq_version_skew_${skew.ui}_${skew.server}`
}
