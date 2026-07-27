import path from 'path'
import { defineConfig } from 'vitest/config'

// Standalone vitest config (instead of a `test` block in vite.config.ts):
// the unit tests don't need the react/tailwind plugins or the rolldown build
// options — only the `@` alias. `node` stays the default environment; the one
// suite that imports browser-touching modules (routes.test.ts) opts into jsdom
// with a `@vitest-environment` docblock.
export default defineConfig({
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.{ts,tsx}'],
  },
})
