import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * The theme is either active or it is not, and nothing else in CI can tell.
 *
 * `@theme` (without `static`) only emits the variables some generated utility
 * literally references. Nothing in this tree writes `bg-brand-500` — Nuxt UI
 * reaches the ramp through its own `primary` alias at runtime — so Tailwind
 * tree-shook the whole palette away and every `bg-primary` in the app resolved
 * to transparent. Every behavioural test stayed green throughout.
 *
 * This is the cheap half of the lesson in `docs/ui-visual-design.md`: assert
 * the three things that make the brand reach the page at all. Not a substitute
 * for looking at it.
 */
const cssPath = join(import.meta.dirname, '..', 'assets', 'css', 'main.css')
const viteConfigPath = join(import.meta.dirname, '..', '..', 'vite.config.ts')

describe('brand theme', () => {
  it('declares the ramp with @theme static so it survives tree-shaking', () => {
    const css = readFileSync(cssPath, 'utf8')
    expect(
      css,
      'plain `@theme` drops the palette unless a utility names a shade — use `@theme static`',
    ).toMatch(/@theme\s+static\s*\{/)
  })

  it('pins 500 to the brand purple from mark-mono.svg', () => {
    const css = readFileSync(cssPath, 'utf8')
    expect(css.toLowerCase()).toContain('--color-brand-500: #6a54df')
  })

  it('points Nuxt UI primary at the brand ramp, not a framework default', () => {
    const config = readFileSync(viteConfigPath, 'utf8')
    expect(config).toMatch(/primary:\s*'brand'/)
  })
})
