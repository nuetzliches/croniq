import { test, expect, login, USER } from './fixtures'

// These specs drive the login form itself, so they use the plain `page`
// fixture — a fresh, signed-out context per test — rather than the shared
// signed-in `app` page.
test.describe('authentication', () => {
  test('an unauthenticated visit lands on the login page', async ({ page }) => {
    await page.goto('/jobs')
    await expect(page).toHaveURL(/\/login/)
  })

  test('password login reaches the dashboard', async ({ page }) => {
    await login(page)
    await expect(page.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()
  })

  test('a wrong password is rejected without navigating', async ({ page }) => {
    await page.goto('/login')
    await page.locator('input[autocomplete="username"]').fill(USER)
    await page.locator('input[autocomplete="current-password"]').fill('not-the-password')
    await page.getByRole('button', { name: /sign in/i }).click()

    await expect(page.getByRole('alert')).toBeVisible()
    await expect(page).toHaveURL(/\/login/)
  })

  /**
   * The behaviour ADR-0001 exists to produce: the access token lives in memory
   * only, so a reload has none — and the session survives anyway, because the
   * dashboard redeems the `HttpOnly` refresh cookie for a fresh one.
   *
   * If this fails, either the cookie stopped being set or the silent refresh
   * on boot stopped running, and every user gets bounced to the login screen
   * on every reload. That is the single most disruptive regression the
   * dashboard can ship, and nothing else in CI would catch it.
   */
  test('a reload keeps the session via the refresh cookie', async ({ page }) => {
    await login(page)

    const cookies = await page.context().cookies()
    const refresh = cookies.find((c) => c.name === 'croniq_refresh')
    expect(refresh, 'refresh cookie must be set on login').toBeDefined()
    expect(refresh!.httpOnly, 'refresh cookie must be HttpOnly (#454)').toBe(true)

    await page.reload()
    await expect(page.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()
    await expect(page).not.toHaveURL(/\/login/)
  })

  /**
   * Logging out is a server round-trip by design (ADR-0001): clearing a cookie
   * locally would leave the refresh token valid for its full seven days, so
   * `POST /v1/auth/logout` revokes it and clears the cookie in one response.
   *
   * The UI fires that request without awaiting it, so this waits on the
   * response rather than on the navigation that races it.
   *
   * Note the menu items are plain `<button>`s inside a `role="menu"` container
   * with no `role="menuitem"`, so `getByRole('menuitem')` finds nothing here.
   *
   * **These two selectors are React-specific and will not match the Vue tree.**
   * `.user-pill` and `.user-menu` are class names from `ui/src/layout/`, and
   * the rebuild's shell uses an accessible name instead. ADR-0004 calls this
   * suite the framework-agnostic acceptance gate, which is true of every other
   * spec here but not of this line — generalising it is cutover work, and
   * saying so here is cheaper than discovering it then.
   */
  test('logging out revokes the session server-side', async ({ page }) => {
    await login(page)

    await page.locator('.user-pill').click()
    const [response] = await Promise.all([
      page.waitForResponse(
        (r) => r.url().includes('/v1/auth/logout') && r.request().method() === 'POST',
      ),
      page.locator('.user-menu').getByRole('button', { name: 'Sign out' }).click(),
    ])
    expect(response.status()).toBeLessThan(400)

    await expect(page).toHaveURL(/\/login$/)

    const cookies = await page.context().cookies()
    expect(cookies.find((c) => c.name === 'croniq_refresh')?.value ?? '').toBe('')
  })
})
