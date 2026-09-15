import { describe, expect, it } from 'vitest'

import { addCollection, addIcon, iconLoaded } from './iconify-offline'

/**
 * The shim that stands in for `@iconify/vue` (ADR-0005).
 *
 * What is under test is not "does storing an icon work" — it is the mismatch
 * that made the offline build silently useless: icons go in keyed
 * `prefix:name`, and Nuxt UI's `Icon.vue` asks for them keyed `prefix-name`.
 * The full build parses the name before looking it up and the offline build
 * does not, so the whole dashboard rendered empty icon slots and nothing
 * anywhere reported an error.
 *
 * `iconLoaded` is the only window this module has onto the offline build's
 * private storage, and it is fed by the same call that writes to it — so
 * asserting through it is asserting on what a lookup would find.
 */
const SQUARE = { body: '<path d="M0 0h24v24H0z"/>' }

describe('iconify-offline', () => {
  it('registers an icon under both the colon and the dashed spelling', () => {
    addIcon('lucide:refresh-cw', SQUARE)

    expect(iconLoaded('lucide:refresh-cw'), 'the name it was added under').toBe(true)
    // The spelling `Icon.vue` actually asks for, having stripped `i-` from
    // `i-lucide-refresh-cw`. This is the assertion the bug would fail.
    expect(iconLoaded('lucide-refresh-cw'), 'the name the component asks for').toBe(true)
  })

  it('splits on the first colon only, so a dashed prefix survives', () => {
    addIcon('simple-icons:github', SQUARE)

    expect(iconLoaded('simple-icons:github')).toBe(true)
    expect(iconLoaded('simple-icons-github')).toBe(true)
  })

  it('leaves a name without a prefix alone', () => {
    addIcon('croniq-mark', SQUARE)

    expect(iconLoaded('croniq-mark')).toBe(true)
  })

  it('reports an icon nobody registered as absent', () => {
    // Not pedantry: this is what tells `Icon.vue` to render its fallback
    // rather than a blank box, and in the offline build there is no request
    // that would fill it in later.
    expect(iconLoaded('lucide-never-added')).toBe(false)
    expect(iconLoaded('lucide:never-added')).toBe(false)
  })

  it('applies both spellings to a collection, aliases included', () => {
    addCollection({
      prefix: 'test',
      icons: { circle: SQUARE },
      aliases: { round: { parent: 'circle' } },
    })

    expect(iconLoaded('test:circle')).toBe(true)
    expect(iconLoaded('test-circle')).toBe(true)
    expect(iconLoaded('test:round')).toBe(true)
    expect(iconLoaded('test-round')).toBe(true)
  })

  it('honours an explicit collection prefix over the set’s own', () => {
    addCollection({ prefix: 'ignored', icons: { dot: SQUARE } }, 'chosen:')

    expect(iconLoaded('chosen:dot')).toBe(true)
    expect(iconLoaded('chosen-dot')).toBe(true)
    expect(iconLoaded('ignored:dot')).toBe(false)
  })
})
