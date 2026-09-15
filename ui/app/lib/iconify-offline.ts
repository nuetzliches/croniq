/**
 * The whole of `@iconify/vue`, as far as this bundle is concerned (ADR-0005).
 *
 * `vite.config.ts` aliases `@iconify/vue` to this module, so every import of
 * it — ours and Nuxt UI's — lands here. The point is that `@iconify/vue/offline`
 * is a different build of the library, not the same one with a flag flipped:
 * it contains no API client, no resource list, and no `api.iconify.design`.
 * A configuration can be lost in a refactor; an export that does not exist
 * cannot be called.
 *
 * What that build drops with the API client is the on-demand path, so every
 * icon has to be in storage before it renders. `vite.config.ts` puts it there
 * (`icon.clientBundle.scan`), and an icon that is missing renders as nothing
 * rather than as a request.
 */
import { Icon, addCollection as addCollectionOffline, addIcon as addIconOffline } from '@iconify/vue/offline'
import type { IconifyIcon, IconifyJSON } from '@iconify/vue/offline'

export { Icon }
export type { IconifyIcon, IconifyJSON }

/**
 * The offline build keeps its icon storage private — it exposes writers
 * (`addIcon`, `addCollection`) and the component that reads them, and nothing
 * to ask. Nuxt UI's `Icon.vue` needs the question answered (`iconLoaded`), so
 * the writers are wrapped and the names recorded on the way past.
 *
 * A mirror of someone else's state is normally a bug waiting to happen. It is
 * safe here because these two functions are the *only* way anything enters
 * that storage in an offline build: there is no loader, no API, and no
 * eviction, so the set cannot drift from the storage it describes.
 */
const loaded = new Set<string>()

/**
 * Register an icon under every spelling something might ask for it by.
 *
 * The two Iconify builds differ here, and the difference is silent. The full
 * one parses the requested name (`stringToIcon`) before looking it up, so
 * `lucide-refresh-cw` and `lucide:refresh-cw` reach the same icon. The offline
 * one is a plain keyed store: `storage[name]`, no parsing. Icons arrive from
 * the bundle keyed `prefix:name`, and Nuxt UI's `Icon.vue` asks for them as
 * `prefix-name` — it strips the leading `i-` from `i-lucide-refresh-cw` and
 * passes the rest straight through.
 *
 * So every icon went in under a key nothing would ever request, the component
 * rendered its empty fallback, and the page looked exactly as it did when the
 * CDN was blocked. Registering both spellings is what makes the offline build
 * a drop-in for the full one.
 */
function register(name: string, data: IconifyIcon): void {
  loaded.add(name)
  addIconOffline(name, data)

  const colon = name.indexOf(':')
  if (colon !== -1) {
    const dashed = `${name.slice(0, colon)}-${name.slice(colon + 1)}`
    loaded.add(dashed)
    addIconOffline(dashed, data)
  }
}

export function addIcon(name: string, data: IconifyIcon): void {
  register(name, data)
}

/**
 * Delegated twice, once per spelling. The offline build's own `addCollection`
 * is what resolves an icon set's aliases and per-set defaults, and that is not
 * worth reimplementing here — but its `prefix` argument is a literal string
 * prepended to every name, so handing it `lucide:` and then `lucide-` writes
 * the same icons under both keys with one implementation.
 *
 * Nothing in this dashboard calls it today — the bundled icons arrive one at a
 * time through Nuxt UI's plugin — but it is part of the module's public shape,
 * and a half-working export is worse than none.
 */
export function addCollection(data: IconifyJSON, prefix?: string | boolean): void {
  const colonPrefix =
    typeof prefix === 'string' ? prefix : prefix !== false && typeof data.prefix === 'string' ? `${data.prefix}:` : ''
  const dashedPrefix = colonPrefix.endsWith(':') ? `${colonPrefix.slice(0, -1)}-` : undefined

  addCollectionOffline(data, colonPrefix)
  if (dashedPrefix !== undefined) {
    addCollectionOffline(data, dashedPrefix)
  }

  for (const name of [...Object.keys(data.icons ?? {}), ...Object.keys(data.aliases ?? {})]) {
    loaded.add(colonPrefix + name)
    if (dashedPrefix !== undefined) {
      loaded.add(dashedPrefix + name)
    }
  }
}

/**
 * Nuxt UI passes this to Iconify's `ssr` prop, which the offline build ignores
 * — this dashboard renders in a browser, never on a server. It is implemented
 * honestly anyway rather than stubbed to `true`: the answer is cheap, and a
 * stub would quietly become wrong the moment anything else asks.
 */
export function iconLoaded(name: string): boolean {
  return loaded.has(name)
}
