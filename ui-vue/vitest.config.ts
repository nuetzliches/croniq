import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vitest/config'

// Standalone, like the React tree's: the unit tests need the `~` alias and
// nothing else — not Vue, not Nuxt UI, not the dev proxy. Component tests will
// need the vue plugin and can add it when the first one exists.
export default defineConfig({
  resolve: {
    alias: { '~': fileURLToPath(new URL('./app', import.meta.url)) },
  },
  test: {
    environment: 'node',
    include: ['app/**/*.test.{ts,tsx}'],
  },
})
