import { test as base, expect, type Page } from '@playwright/test'

/** Matches `scripts/e2e-stack.mjs`. Public demo credentials by design. */
export const USER = 'admin'
export const PASSWORD = 'demo-admin'

/**
 * Log in through the real form.
 *
 * Not a seeded token or an injected cookie: since #454 the refresh token
 * arrives as an `HttpOnly` cookie the page cannot set, so a shortcut here
 * would test a session shape production never produces — and the reload
 * behaviour in `auth.spec.ts` is precisely what that cookie decides.
 *
 * The fields carry no accessible name (the login form's `<label>` elements
 * have neither `htmlFor` nor a nested input), so these select on the
 * autocomplete attributes instead. When that is fixed, switch to
 * `getByLabel` — the selectors here are a workaround, not a preference.
 */
export async function login(page: Page) {
  await page.goto('/login')
  await page.locator('input[autocomplete="username"]').fill(USER)
  await page.locator('input[autocomplete="current-password"]').fill(PASSWORD)

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
 *
 * Tests share this page, so anything a test toggles it must toggle back.
 */
// The first slot is Playwright's per-test fixtures, of which this file adds
// none. It has to be an empty object type rather than `Record<string, never>`:
// the latter's index signature forces every fixture — `app` included — to
// `never`, and the extend call stops type checking.
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export const test = base.extend<{}, { app: Page }>({
  app: [
    async ({ browser }, use) => {
      const context = await browser.newContext()
      const page = await context.newPage()
      await login(page)
      await use(page)
      await context.close()
    },
    { scope: 'worker' },
  ],
})

export { expect }
