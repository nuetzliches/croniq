import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import ui from '@nuxt/ui/vite'

/**
 * The Vue dashboard (ADR-0004). Lives beside `ui/` rather than replacing it:
 * React ships until this passes the acceptance gate, and both dev servers run
 * at once so the two can be compared — see `scripts/dev-stack.mjs`.
 *
 * Port 5174, because `ui/` holds 5173.
 *
 * There is deliberately no `VITE_API_URL` escape hatch here, unlike the React
 * tree. That flag exists there for cross-origin builds and needs a guard to
 * stop it silently downgrading refresh-token storage (ADR-0001). Rather than
 * port the guard, this build is same-origin only: `croniq-server --ui-dir` and
 * the `croniq-ui` container both serve it same-origin, which is the supported
 * topology. If a cross-origin build is ever needed, it needs the guard back
 * with it, not the flag alone.
 */
const API_ORIGIN = process.env.CRONIQ_API_ORIGIN ?? 'http://localhost:4000'

export default defineConfig({
  plugins: [
    vue(),
    ui({
      ui: {
        colors: {
          primary: 'blue',
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
    }),
  ],
  resolve: {
    alias: {
      '~': fileURLToPath(new URL('./app', import.meta.url)),
    },
  },
  server: {
    port: 5174,
    // See ui/vite.config.ts: a silent move to another port makes the
    // side-by-side comparison compare the wrong things.
    strictPort: true,
    // Same paths the React tree proxies. This is what makes `npm run dev`
    // same-origin, so the refresh cookie works in development exactly as it
    // does in production (ADR-0001).
    proxy: {
      '/v1': API_ORIGIN,
      '/health': API_ORIGIN,
      '/version': API_ORIGIN,
      '/metrics': API_ORIGIN,
    },
  },
})
