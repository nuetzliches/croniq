import { describe, expect, it } from 'vitest'
import { skewDismissalKey, versionSkew } from './build-version'

describe('versionSkew', () => {
  it('reports nothing when the versions match', () => {
    expect(versionSkew('0.38.0', '0.38.0')).toBeNull()
  })

  it('reports both versions when they differ', () => {
    expect(versionSkew('0.39.0', '0.38.0')).toEqual({ ui: '0.39.0', server: '0.38.0' })
  })

  /**
   * The failure this guards against is the guard itself being wrong. Release
   * tags are `v0.38.0` and `CARGO_PKG_VERSION` is `0.38.0`, so a naive string
   * compare would report a permanent skew on a correctly matched pair — a
   * warning that is wrong every time, until someone turns it off for good.
   */
  it('does not mistake a v-prefix for a mismatch', () => {
    expect(versionSkew('0.38.0', 'v0.38.0')).toBeNull()
  })

  /**
   * An unstamped bundle says nothing. Every `npm run build` on a laptop and
   * every non-release CI build lands here, and a dev build that nags on each
   * reload is how a warning gets ignored on the day it is real.
   */
  it.each([null, '', '   ', 'dev', 'unknown'])('stays silent for a ui version of %o', (ui) => {
    expect(versionSkew(ui as string | null, '0.38.0')).toBeNull()
  })

  /** Same in the other direction: an older server without `/version`. */
  it.each([null, undefined, '', 'unknown'])('stays silent for a server version of %o', (server) => {
    expect(versionSkew('0.39.0', server)).toBeNull()
  })

  it('compares exactly, so a patch-level difference still counts', () => {
    expect(versionSkew('0.38.1', '0.38.0')).toEqual({ ui: '0.38.1', server: '0.38.0' })
  })
})

describe('skewDismissalKey', () => {
  it('is specific to the pair, so a new mismatch is not pre-dismissed', () => {
    const first = skewDismissalKey({ ui: '0.39.0', server: '0.38.0' })
    const second = skewDismissalKey({ ui: '0.39.0', server: '0.37.0' })
    expect(first).not.toBe(second)
  })

  it('is stable for the same pair', () => {
    expect(skewDismissalKey({ ui: '0.39.0', server: '0.38.0' })).toBe(
      skewDismissalKey({ ui: '0.39.0', server: '0.38.0' }),
    )
  })
})
