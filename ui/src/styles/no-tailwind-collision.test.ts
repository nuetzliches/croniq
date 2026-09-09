import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * The stylesheets under `src/styles/` are plain, unlayered CSS. Tailwind v4
 * emits its utilities inside `@layer utilities`, and unlayered CSS beats
 * layered CSS in the cascade regardless of source order and regardless of
 * equal specificity.
 *
 * So a rule here named after a Tailwind utility does not merely compete with
 * it — it silently and unconditionally wins. That is what #584 was: `.grid`
 * (with `gap: 14px`), `.gap-4…20` in pixels, and `.grow` shadowed their
 * Tailwind namesakes at 64 call sites. `class="grid grid-cols-7 gap-1"` read
 * as Tailwind and rendered as something else, and nothing said so.
 *
 * The fix was to rename them to `cq-*`. This keeps them renamed.
 */
const STYLES_DIR = join(import.meta.dirname)

/**
 * Utility names Tailwind emits that a component stylesheet might plausibly
 * reach for. Not the full Tailwind surface — that is thousands of generated
 * names and most of them (`pt-3.5`, `z-40`) nobody would hand-write here.
 * These are the ones with an obvious layout or presentational meaning, which
 * is exactly why they are tempting and why the collision happened.
 */
const RESERVED = [
  // display
  'block', 'inline', 'inline-block', 'flex', 'inline-flex', 'grid', 'inline-grid',
  'contents', 'hidden', 'table',
  // flex/grid children
  'grow', 'shrink', 'basis', 'order',
  // box
  'border', 'rounded', 'shadow', 'container', 'isolate',
  // position
  'static', 'fixed', 'absolute', 'relative', 'sticky',
  // text
  'truncate', 'uppercase', 'lowercase', 'capitalize', 'italic', 'underline',
  // a11y
  'sr-only',
]

/** Scale-suffixed families: `gap-4`, `p-2`, `m-1.5`, `w-64`, … */
const RESERVED_PREFIXES = [
  'gap', 'gap-x', 'gap-y', 'p', 'px', 'py', 'pt', 'pr', 'pb', 'pl',
  'm', 'mx', 'my', 'mt', 'mr', 'mb', 'ml', 'w', 'h', 'text', 'bg', 'z',
  'flex', 'grid-cols', 'col-span', 'row-span', 'opacity', 'gap',
]

function cssFiles(): string[] {
  return readdirSync(STYLES_DIR)
    .filter((f) => f.endsWith('.css'))
    .map((f) => join(STYLES_DIR, f))
}

/**
 * Top-level class names a stylesheet defines.
 *
 * Deliberately crude — every `.name` token in a selector position. Over-
 * reporting is harmless here (the assertion is "none of these are Tailwind
 * names"); under-reporting would not be.
 */
function definedClasses(css: string): Set<string> {
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, '')
  const names = new Set<string>()
  for (const match of withoutComments.matchAll(/\.(-?[A-Za-z_][\w-]*)/g)) {
    names.add(match[1])
  }
  return names
}

function isReserved(name: string): boolean {
  if (RESERVED.includes(name)) return true
  return RESERVED_PREFIXES.some((prefix) => new RegExp(`^${prefix}-[\\d.]+$`).test(name))
}

describe('unlayered stylesheets do not shadow Tailwind utilities', () => {
  for (const file of cssFiles()) {
    it(`${file.split(/[\\/]/).pop()} defines no Tailwind utility name`, () => {
      const offenders = [...definedClasses(readFileSync(file, 'utf8'))]
        .filter(isReserved)
        .sort()

      expect(
        offenders,
        offenders.length
          ? `These class names shadow Tailwind utilities and will win over them ` +
            `unconditionally, because this file is unlayered. Rename them (the ` +
            `existing convention is a "cq-" prefix) rather than relying on source ` +
            `order, which does not decide this.`
          : undefined,
      ).toEqual([])
    })
  }
})
