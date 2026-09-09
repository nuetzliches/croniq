import { defineConfig, devices } from '@playwright/test'

/**
 * End-to-end smoke suite (issue #586).
 *
 * Scope is deliberately "would anyone notice if this broke", not coverage.
 * `tsc` and the unit tests already catch what they can catch; what neither can
 * tell you is whether logging in still works, so that is what lives here.
 *
 * The stack comes up through `scripts/e2e-stack.mjs` rather than being assumed
 * to exist, so a laptop run and a CI run are the same run. `reuseExistingServer`
 * is off in CI and on locally, where re-seeding a database for every `--watch`
 * iteration would be the slowest part of the loop.
 */
const PORT = Number(process.env.CRONIQ_E2E_PORT ?? 4010)
const BASE_URL = `http://127.0.0.1:${PORT}`

export default defineConfig({
  testDir: './e2e',
  // Serial by default. The suite shares one server and one database, and the
  // dashboard's own state (theme, sidebar, the selected job) is per-profile
  // rather than per-tab — parallel workers would race over it for no gain on a
  // suite this size.
  workers: 1,
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  // One retry in CI only. Locally a flake should be seen, not papered over.
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : [['list']],
  timeout: 30_000,
  expect: { timeout: 10_000 },

  use: {
    baseURL: BASE_URL,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'off',
  },

  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],

  webServer: {
    command: 'node scripts/e2e-stack.mjs',
    url: `${BASE_URL}/health`,
    reuseExistingServer: !process.env.CI,
    // The server seeds a database and the runner has to register before the
    // Runners page has anything to show; a debug build takes its time doing it.
    timeout: 120_000,
    stdout: 'pipe',
    stderr: 'pipe',
  },
})
