import path from 'path'
import { defineConfig } from 'vitest/config'

// Standalone vitest config (instead of a `test` block in vite.config.ts):
// the unit tests are plain TypeScript, so they don't need the react/
// tailwind plugins or the rolldown build options — only the `@` alias.
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
