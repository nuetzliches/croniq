import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * `index.html` carries a meta-tag copy of the server's Content-Security-Policy,
 * for a `dist/` served by a web server that sends none (review 2026-10-24,
 * U-7). A browser enforces the header and the meta policy together, so a copy
 * that drifted would either block what the header allows or quietly be the
 * looser of the two on a deployment without the header. Both directions are
 * checked against the Rust constant, the source of truth.
 */
const root = join(import.meta.dirname, '..', '..')
const indexHtml = join(root, 'index.html')
const hardeningRs = join(root, '..', 'crates', 'croniq-server', 'src', 'api', 'hardening.rs')

/** Browsers ignore these in a `<meta>` policy and warn about them. */
const HEADER_ONLY = ['frame-ancestors', 'report-uri', 'sandbox']

function directives(policy: string): string[] {
  return policy
    .split(';')
    .map((directive) => directive.trim().replace(/\s+/g, ' '))
    .filter(Boolean)
}

function serverPolicy(): string {
  const source = readFileSync(hardeningRs, 'utf8').replace(/\r\n/g, '\n')
  const match = /pub const CONTENT_SECURITY_POLICY: &str = "((?:[^"\\]|\\.)*)";/s.exec(source)
  if (!match) throw new Error('CONTENT_SECURITY_POLICY not found in hardening.rs')
  // Rust's `\` line continuation drops the newline and the next line's
  // leading whitespace.
  return match[1]!.replace(/\\\n\s*/g, '')
}

function metaPolicy(): string {
  const html = readFileSync(indexHtml, 'utf8')
  const match =
    /<meta\s+http-equiv="Content-Security-Policy"\s+content="([^"]*)"/i.exec(html)
  if (!match) throw new Error('index.html carries no Content-Security-Policy meta tag')
  return match[1]!
}

describe('meta CSP fallback in index.html', () => {
  it('is the server policy minus the header-only directives', () => {
    const expected = directives(serverPolicy()).filter(
      (directive) => !HEADER_ONLY.includes(directive.split(' ')[0]!),
    )
    expect(expected.length).toBeGreaterThan(0)
    expect(directives(metaPolicy())).toEqual(expected)
  })

  it('comes before any script or stylesheet in <head>', () => {
    // A meta policy governs only what the parser meets after it.
    const html = readFileSync(indexHtml, 'utf8')
    const meta = html.search(/<meta\s+http-equiv="Content-Security-Policy"/i)
    const firstLoad = html.search(/<script|<link rel="(?:stylesheet|modulepreload)"/i)
    expect(meta).toBeGreaterThan(-1)
    if (firstLoad !== -1) expect(meta).toBeLessThan(firstLoad)
  })
})
