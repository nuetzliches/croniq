import { fileURLToPath, URL } from 'node:url'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vitest/config'

// Standalone, like the React tree's: no Nuxt UI, no dev proxy, no auto-imports.
//
// The vue plugin is here because the router's routes are lazy imports of `.vue`
// files, so anything that navigates has to be able to parse one — which the
// auth-watch tests do (issue #665). It does not make this a component-test
// setup: there is still no DOM by default and no `@vue/test-utils`. A test that
// needs a document asks for one per file with `// @vitest-environment happy-dom`.
export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: { '~': fileURLToPath(new URL('./app', import.meta.url)) },
  },
  test: {
    environment: 'node',
    include: ['app/**/*.test.{ts,tsx}'],
  },
})
