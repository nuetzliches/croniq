import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vite'
import { PORTS } from '../scripts/lib/stack.mjs'
import vue from '@vitejs/plugin-vue'
import ui from '@nuxt/ui/vite'

/**
 * The dashboard (ADR-0004). Port 4231, the UI slot in croniq's 4230-4233
 * development block — see `../scripts/dev-stack.mjs`.
 *
 * There is deliberately no `VITE_API_URL` escape hatch. The dashboard this one
 * replaced had one, for cross-origin builds, and it needed a build-time guard
 * (`assertTokenStorageAcknowledged`) to stop it silently downgrading
 * refresh-token storage from an `HttpOnly` cookie to `localStorage`
 * (ADR-0001). Rather than port the flag and the guard, this build is
 * same-origin only: `croniq-server --ui-dir` and the `croniq-ui` container
 * both serve it same-origin, which is the supported topology. An absent flag
 * needs no guard. If a cross-origin build is ever wanted, it needs the guard
 * back with it, not the flag alone.
 */
const API_ORIGIN = process.env.CRONIQ_API_ORIGIN ?? `http://127.0.0.1:${PORTS.api}`

export default defineConfig({
  plugins: [
    vue(),
    ui({
      ui: {
        colors: {
          // The product's own purple (see app/assets/css/main.css), not a
          // framework default. `primary: 'blue'` was a placeholder I set
          // without flagging it as a decision.
          primary: 'brand',
          neutral: 'slate',
          success: 'green',
          info: 'blue',
          warning: 'amber',
          error: 'red',
        },
      },
      // Replaces Nuxt's auto-imports. Kept to the dirs that exist rather than
      // a speculative list: an auto-import dir that does not exist is a silent
      // no-op, and a reader cannot tell it from one that is simply empty.
      autoImport: {
        vueTemplate: true,
        dirs: ['app/composables', 'app/stores'],
        imports: ['vue', 'vue-router', 'pinia', '@vueuse/core'],
      },
      components: { dirs: ['app/components'] },
      // Icons are resolved here, at build time, and never over the network
      // (ADR-0005). Nuxt UI bundles the icons named in its own app config by
      // default; the ~70 `i-lucide-*` names this dashboard writes itself were
      // not among them, so each one became a runtime request to
      // `api.iconify.design` — blocked by the server's `connect-src 'self'`,
      // and on a deployment without that header, a page view reported to a
      // third party.
      //
      // `globInclude` is spelled out because the default
      // (`**/*.{vue,jsx,tsx,md,…}`) does not include `.ts`, and
      // `app/router/nav.ts` is where the sidebar's icons live — exactly the
      // ones that were missing. Scoped to `app/` so the scan does not walk
      // `dist/` or the e2e suite.
      icon: {
        clientBundle: {
          scan: { globInclude: ['app/**/*.{vue,ts}'] },
        },
      },
    }),
  ],
  resolve: {
    // The array form, not the object shorthand, because one of these has to
    // match exactly. A string alias also matches everything under it
    // (`find + '/'`), which for `@iconify/vue` would rewrite the shim's own
    // `@iconify/vue/offline` import into a path inside itself.
    alias: [
      { find: '~', replacement: fileURLToPath(new URL('./app', import.meta.url)) },
      // The offline build of Iconify, via a shim that reconciles it with what
      // Nuxt UI expects (ADR-0005; `app/lib/iconify-offline.ts` says how).
      // Bundling every icon (above) means nothing *needs*
      // the API client; this alias means nothing *has* it — `api.iconify.design`
      // is not a host the shipped JS knows about, so no later config change can
      // reintroduce the fetch. Verified by `scripts/check-local-sources.mjs`.
      {
        find: /^@iconify\/vue$/,
        replacement: fileURLToPath(new URL('./app/lib/iconify-offline.ts', import.meta.url)),
      },
    ],
  },
  server: {
    port: PORTS.ui,
    // Refuse rather than silently move: vite's default is to take the next
    // free port, which prints a URL that nothing the dev stack advertises is
    // listening on. `dev-stack.mjs` checks the port up front for the same
    // reason and answers with the variable to set.
    strictPort: true,
    // Proxying these is what makes `npm run dev` same-origin, so the refresh
    // cookie works in development exactly as it does in production
    // (ADR-0001).
    proxy: {
      '/v1': API_ORIGIN,
      '/health': API_ORIGIN,
      '/version': API_ORIGIN,
      '/metrics': API_ORIGIN,
    },
  },
})
