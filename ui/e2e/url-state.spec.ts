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

  test('a settings tab is selected by ?tab', async ({ app }) => {
    await app.goto('/settings?tab=clients')
    await expect(app.locator('.tab.active')).toHaveText(/api|client/i)
  })

  /**
   * `?tab` is whitelisted against the known tabs and falls back to `profile`.
   * Without that, an unknown value renders no tab body at all — a blank panel
   * inside an otherwise working page, which is the kind of thing that reaches
   * production because it only happens on a hand-edited URL.
   */
  test('an unknown ?tab falls back to the profile tab', async ({ app }) => {
    await app.goto('/settings?tab=not-a-tab')
    await expect(app.locator('.tab.active')).toHaveText(/profile/i)
  })

  test('a job deep link opens that job', async ({ app }) => {
    await app.goto('/jobs/demo%3Aheartbeat')
    await expect(app.getByText('demo:heartbeat').first()).toBeVisible()
  })

  /**
   * `/jobs` with no key selects the first job implicitly. Dropping that leaves
   * the detail pane empty on the page most operators open first.
   */
  test('the jobs list selects a job when none is named', async ({ app }) => {
    await app.goto('/jobs')
    await expect(app.locator('.job-row, [data-job-key], tbody tr').first()).toBeVisible()
  })
})
