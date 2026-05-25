import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
    // Cases set their own `duration_max_ms`; this is just a safety net so a
    // runaway binding doesn't hang CI.
    testTimeout: 30_000,
    hookTimeout: 30_000,
  },
});
