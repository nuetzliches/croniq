import { defineConfig, loadEnv } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

// API base URL for the dev proxy. The default of "" in api/base.ts means
// "same origin" — fine for production (UI + API share the origin), but in
// dev the UI runs on :5173 and the API on :4000, so we need to forward the
// API paths. Override via CRONIQ_API_ORIGIN if you run the server on a
// different host/port while developing the UI.
const API_ORIGIN = process.env.CRONIQ_API_ORIGIN ?? 'http://localhost:4000'

/** Env var that acknowledges the weaker cross-origin token storage. */
const ACK = 'VITE_ALLOW_LOCALSTORAGE_REFRESH'

/**
 * Refuse to produce a bundle that keeps the refresh token in `localStorage`
 * without someone having said so (issue #454).
 *
 * A same-origin build gets the refresh token as an `HttpOnly` cookie, out of
 * reach of any XSS. `VITE_API_URL` points the dashboard at a *different*
 * origin, where a `SameSite=Strict` cookie can never be delivered, so that
 * build has to fall back to `localStorage` — the exposure #454 exists to
 * remove. That trade is occasionally the right call, but it must be a choice
 * rather than a side effect of setting an unrelated-looking variable.
 *
 * Enforced here rather than at runtime so the error reaches the person running
 * the build instead of an end user staring at a broken dashboard. Note that
 * dev needs none of this: the Vite proxy below serves /v1 from the dev
 * server's own origin, so `npm run dev` is same-origin and gets the cookie.
 */
function assertTokenStorageAcknowledged(mode: string) {
  // `loadEnv` (not `process.env`) so .env files count — that is where a
  // VITE_API_URL usually lives.
  const env = loadEnv(mode, __dirname, '')
  if (!env.VITE_API_URL) return
  if (env[ACK] === '1') return
  throw new Error(
    `VITE_API_URL is set (${env.VITE_API_URL}), which builds the dashboard for a\n` +
      `different origin than the API. A SameSite=Strict refresh cookie cannot reach\n` +
      `that origin, so this build would keep the refresh token in localStorage,\n` +
      `where any XSS can read it and hold the account for the token's full 7-day\n` +
      `life (issue #454).\n\n` +
      `Either:\n` +
      `  * drop VITE_API_URL and serve the dashboard from croniq-server itself\n` +
      `    (the default, and what the official Docker image does), or\n` +
      `  * set ${ACK}=1 to build anyway, accepting localStorage storage.\n\n` +
      `See docs/operations.md → "Where the dashboard keeps its tokens".`,
  )
}

export default defineConfig(({ mode }) => {
  assertTokenStorageAcknowledged(mode)
  return {
    plugins: [react(), tailwindcss()],
    resolve: {
      alias: {
        '@': path.resolve(__dirname, './src'),
      },
    },
    build: {
      rolldownOptions: {
        output: {
          codeSplitting: {
            groups: [
              { name: 'charts', test: /node_modules\/recharts/ },
              { name: 'radix', test: /node_modules\/@radix-ui/ },
              { name: 'react', test: /node_modules\/(react|react-dom|scheduler)\// },
              { name: 'router', test: /node_modules\/react-router/ },
              { name: 'query', test: /node_modules\/@tanstack/ },
              { name: 'icons', test: /node_modules\/lucide-react/ },
              { name: 'forms', test: /node_modules\/react-hook-form/ },
            ],
          },
        },
      },
    },
    server: {
      proxy: {
        '/v1': API_ORIGIN,
        '/health': API_ORIGIN,
        '/version': API_ORIGIN,
        '/metrics': API_ORIGIN,
      },
    },
  }
})
