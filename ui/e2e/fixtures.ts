import { test as base, expect, type Page } from '@playwright/test'

/** Matches `scripts/e2e-stack.mjs`. Public demo credentials by design. */
export const USER = 'admin'
export const PASSWORD = 'demo-admin'
/**
 * The stack's seeded API key, `admin`-scoped (`croniq init --api-key`).
 * Matches `scripts/e2e-stack.mjs`. For setting up what a spec needs through
 * the API — a second user, say — without driving the dashboard to do it.
 */
export const API_KEY = 'croniq_e2e_local_suite_key_not_for_production_use'

/**
 * Log in through the real form.
 *
 * Not a seeded token or an injected cookie: since #454 the refresh token
 * arrives as an `HttpOnly` cookie the page cannot set, so a shortcut here
 * would test a session shape production never produces — and the reload
 * behaviour in `auth.spec.ts` is precisely what that cookie decides.
 *
 * The fields are selected by label. That was not possible against the
 * dashboard this one replaces — its login `<label>`s had neither `htmlFor` nor
 * a nested input, so the fields carried no accessible name at all (#595) and
 * the suite reached them through their `autocomplete` attributes instead. Now
 * that the names are real, naming them here is worth more than a CSS selector:
 * it fails if they ever stop being real.
 */
export async function login(page: Page) {
  await page.goto('/login')
  await page.getByLabel('Username', { exact: true }).fill(USER)
  await page.getByLabel('Password', { exact: true }).fill(PASSWORD)

  // Wait on the response, not on a URL change. "no longer /login" is not a
  // usable signal — a pattern loose enough to accept every post-login route
  // also matches `http://…` itself, and the helper then returns before the
  // request has even been answered. Everything that reads session state
  // afterwards (the refresh cookie above all) races it.
  const [response] = await Promise.all([
    page.waitForResponse(
      (r) => r.url().includes('/v1/auth/login') && r.request().method() === 'POST',
    ),
    page.getByRole('button', { name: /sign in/i }).click(),
  ])
  expect(response.status(), 'login must succeed').toBe(200)

  await expect(page.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()
}

/** This origin's `localStorage`, or `null` where the page has none to read. */
async function readStorage(page: Page): Promise<Record<string, string> | null> {
  try {
    return await page.evaluate(() => {
      // Sorted, so two reads of the same contents compare equal as JSON.
      const keys = Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)!).sort()
      return Object.fromEntries(keys.map((key) => [key, localStorage.getItem(key)!]))
    })
  } catch {
    // `about:blank`, a closed page, a navigation in progress: nothing to keep.
    return null
  }
}

/**
 * One signed-in page, shared by every spec in the worker.
 *
 * The obvious alternative — a `storageState` file written by a setup project —
 * does not work against this server, and the way it fails is worth recording.
 * `POST /v1/auth/refresh` **rotates**: it revokes the presented refresh token
 * and issues a new one (`auth_endpoints.rs`, "Revoke old token"). A saved
 * `storageState` is therefore single-use. The first test to boot from it
 * redeems the cookie and burns it; every later test replays a revoked token
 * and lands on the login page. That reads as a broken suite rather than as a
 * correctly rotating token, which is exactly the wrong signal to leave lying
 * around.
 *
 * A worker-scoped context keeps the rotation chain continuous, the way a real
 * browser tab does, and costs one login per run instead of one per test —
 * which also keeps the suite clear of the per-IP login throttle (30 attempts
 * per five minutes, #428) as it grows.
 */
// The first slot is the per-test fixtures (`app`), the second the per-worker
// ones (`signedIn`).
export const test = base.extend<{ app: Page }, { signedIn: Page }>({
  signedIn: [
    async ({ browser }, use) => {
      const context = await browser.newContext()
      const page = await context.newPage()
      await login(page)
      await use(page)
      await context.close()
    },
    { scope: 'worker' },
  ],

  /**
   * The shared page, put back the way the test found it.
   *
   * Tests share one page, so a preference one of them changes — theme,
   * sidebar, the timeline's lane order or hover hold, all per-browser and kept
   * in `localStorage` — reaches every test after it. Each spec used to toggle
   * its changes back by hand at the end, which a failing assertion skipped:
   * the next test inherited the state and failed for a reason of someone
   * else's, and the CI retry hid it (review 2026-10-24, U-5).
   *
   * Restored here, after every test whether it passed or not. Only when
   * something changed is the page reloaded, so the app re-reads what it keeps
   * in memory; tests that change nothing pay nothing. A second tab a test
   * opened is closed, and the pointer leaves whatever it was hovering.
   *
   * Server-side state (favorites, notes) is not covered: a spec that creates
   * it still removes it in a `finally` or an `afterAll`.
   */
  app: async ({ signedIn: page }, use) => {
    const saved = await readStorage(page)
    await use(page)

    for (const other of page.context().pages()) {
      if (other !== page) await other.close()
    }
    if (page.isClosed() || saved === null) return
    await page.mouse.move(0, 0)
    const now = await readStorage(page)
    if (now === null || JSON.stringify(now) === JSON.stringify(saved)) return
    await page.evaluate((entries) => {
      localStorage.clear()
      for (const [key, value] of Object.entries(entries)) localStorage.setItem(key, value)
    }, saved)
    await page.reload()
  },
})

export { expect }
