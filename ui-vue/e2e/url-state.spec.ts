import { test, expect } from './fixtures'

/**
 * The parts of the dashboard's state that live in the URL, and therefore in
 * bookmarks, shared links and the back button.
 *
 * These are contracts with people, not with code: a filter that silently stops
 * round-tripping through `?state=` breaks every link anyone has pasted into a
 * ticket, and nothing else in CI can see it happen.
 */
test.describe('URL state', () => {
  test('an executions filter is readable from the URL', async ({ app }) => {
    await app.goto('/executions?state=succeeded')
    await expect(app).toHaveURL(/state=succeeded/)
    await expect(app.getByRole('navigation', { name: 'Main navigation' })).toBeVisible()

    // Reload: the filter must be restored from the URL, not from memory.
    await app.reload()
    await expect(app).toHaveURL(/state=succeeded/)
  })

  test('multiple executions filters coexist in the query string', async ({ app }) => {
    await app.goto('/executions?state=succeeded&job_key=demo%3Aheartbeat')
    await expect(app).toHaveURL(/state=succeeded/)
    await expect(app).toHaveURL(/job_key=demo%3Aheartbeat/)
  })

  test('a settings sub-view is addressable', async ({ app }) => {
    await app.goto('/settings/clients')
    // What the URL promises is that this view is *shown*, so that is what is
    // asserted — not that a tab carries `aria-selected`. The React tabs are
    // styled `<button>`s with no `role="tab"` at all, and testing the chrome
    // rather than the content would have made this assertion about markup the
    // rebuild is replacing anyway.
    await expect(app.getByText(/api client/i).first()).toBeVisible()
  })

  /**
   * A sub-view nobody defined must not render a blank panel inside an
   * otherwise working page — the kind of thing that reaches production because
   * it only happens on a hand-edited URL. React whitelists `?tab` and falls
   * back to the profile; the Vue tree constrains the path segment, so an
   * unknown one is not a settings URL at all and the router says so.
   */
  test('an unknown settings sub-view does not render a blank panel', async ({ app }) => {
    await app.goto('/settings/not-a-section')
    await expect(app.getByText(/not found/i).first()).toBeVisible()
  })

  test('a job deep link opens that job', async ({ app }) => {
    await app.goto('/jobs/demo%3Aheartbeat')
    await expect(app.getByText('demo:heartbeat').first()).toBeVisible()
  })

  /**
   * `/jobs` with no key still lists jobs.
   *
   * The React tree also *selected* the first one implicitly; the Vue tree
   * deliberately does not, because a list that opens a detail you did not ask
   * for is a list you have to close before you can read it. What both owe is
   * a populated list — that is what is asserted.
   */
  test('the jobs list is populated when no job is named', async ({ app }) => {
    await app.goto('/jobs')
    await expect(app.locator('.job-row, [data-job-key], tbody tr').first()).toBeVisible()
  })

  /**
   * Opening a detail keeps the list's filters in the URL.
   *
   * Only the Vue tree publishes this uniformly: its detail routes carry the
   * list's query forward, so a filtered view survives being navigated into and
   * a shared link still describes what the sender was looking at.
   *
   * Asserted on the jobs list rather than the runs list, deliberately. Runs
   * would be the more natural place — filters matter most there — but a freshly
   * seeded stack has no executions for the first minute, so the assertion
   * would be about how long the suite happened to take. Jobs come from the
   * Croniqfile and exist the moment the server boots. Same promise, data that
   * is there.
   */
  test('opening a detail keeps the list filters in the URL', async ({ app }) => {
    await app.goto('/jobs?q=demo')
    const row = app.locator('tbody tr').first()
    await expect(row).toBeVisible()
    await row.click()
    await expect.poll(() => new URL(app.url()).pathname).toMatch(/^\/jobs\/.+/)
    await expect(app).toHaveURL(/q=demo/)
  })
})
