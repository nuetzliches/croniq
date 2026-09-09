import { test, expect } from './fixtures'

/**
 * The routes reachable from the sidebar, and the URL contracts the dashboard
 * publishes. A broken route here is invisible to `tsc` — the route table type
 * checks perfectly while pointing at a component that throws on mount.
 */
test.describe('navigation', () => {
  for (const { label, path } of [
    { label: 'Dashboard', path: '/' },
    { label: 'Jobs', path: '/jobs' },
    { label: 'Executions', path: '/executions' },
    { label: 'Runners', path: '/runners' },
    { label: 'Dead Letters', path: '/dead-letters' },
    { label: 'Alerts', path: '/alerts' },
    { label: 'Calendars', path: '/calendars' },
    { label: 'Settings', path: '/settings' },
  ]) {
    test(`the ${label} page loads without an error boundary`, async ({ app }) => {
      const errors: string[] = []
      app.on('pageerror', (e) => errors.push(e.message))

      await app.getByRole('navigation', { name: 'Main navigation' }).getByRole('link', { name: label }).click()

      // Compare the pathname directly rather than building a regex out of it.
      // A hand-rolled escape is both unnecessary (a forward slash needs none
      // inside `RegExp`) and wrong (it left backslashes alone) — CodeQL's
      // js/incomplete-sanitization, and it was right.
      await expect.poll(() => new URL(app.url()).pathname).toBe(path)
      // The shell must survive the navigation — if the page component throws,
      // React unmounts the tree and the nav goes with it.
      await expect(app.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()
      expect(errors, `uncaught errors on ${path}`).toEqual([])
    })
  }

  test('an unknown route renders the not-found page, not a blank shell', async ({ app }) => {
    await app.goto('/no-such-page')
    await expect(app.getByText(/not found/i).first()).toBeVisible()
  })

  /**
   * The SPA fallback contract: a deep link typed into the address bar is a
   * document request for a path that has no file behind it. The server answers
   * `index.html` and the router takes over. This breaks the moment someone
   * puts a `try_files … =404` in front of the dashboard, and #587 proposes
   * exactly that kind of change.
   */
  test('a deep link entered directly is served by the SPA fallback', async ({ app }) => {
    const response = await app.goto('/executions')
    expect(response?.status()).toBe(200)
    await expect(app).toHaveURL(/\/executions$/)
    await expect(app.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()
  })
})
