import { test, expect } from './fixtures'

/**
 * The two SSE surfaces and the preferences that must survive a reload.
 *
 * These are the scenarios worth the most: the SSE clients are hand-rolled
 * (#585), and the console's buffer is the single riskiest thing to port to a
 * different reactivity model (#589). A regression in either is silent — the
 * page renders, it just stops updating.
 */
test.describe('live surfaces', () => {
  test('the runners page shows the connected demo runner', async ({ app }) => {
    await app.goto('/runners')
    // The stack attaches one demo runner with RUNNER_TAGS=env=e2e.
    await expect(app.getByText('env=e2e').first()).toBeVisible({ timeout: 20_000 })
  })

  test('the runners stream is opened as SSE', async ({ app }) => {
    const streamed = app.waitForResponse(
      (r) => r.url().includes('/v1/runners/stream') || r.request().headers()['accept'] === 'text/event-stream',
      { timeout: 20_000 },
    )
    await app.goto('/runners')
    const response = await streamed
    expect(response.status()).toBe(200)
  })

  /**
   * Asserts events actually arrive, not merely that the stream opens with a
   * 200. "The page renders, it just stops updating" is the failure mode this
   * describe block exists for, and a status-code check cannot see it — which
   * is why the stack runs the server at `RUST_LOG=info`.
   */
  test('the console page receives log events', async ({ app }) => {
    const streamed = app.waitForResponse(
      (r) => r.request().headers()['accept'] === 'text/event-stream',
      { timeout: 20_000 },
    )
    await app.goto('/console')
    const response = await streamed
    expect(response.status()).toBe(200)

    // The header reads "<filtered> / <total> events".
    await expect
      .poll(
        async () => {
          const text = (await app.getByText(/\d+ \/ \d+ events?/).first().textContent()) ?? ''
          return Number(text.match(/\/\s*(\d+)/)?.[1] ?? 0)
        },
        { timeout: 20_000 },
      )
      .toBeGreaterThan(0)
  })
})

test.describe('preferences survive a reload', () => {
  test('the theme choice persists', async ({ app }) => {
    await app.locator('.user-pill').click()
    await app.locator('.user-menu').getByRole('radio', { name: /light/i }).click()
    await expect(app.locator('html')).toHaveAttribute('data-theme', 'light')

    await app.reload()
    await expect(app.locator('html')).toHaveAttribute('data-theme', 'light')

    // Restore, so a later test does not inherit a non-default theme.
    await app.locator('.user-pill').click()
    await app.locator('.user-menu').getByRole('radio', { name: /dark/i }).click()
  })

  test('the collapsed sidebar persists', async ({ app }) => {
    const shell = app.locator('.app')
    await expect(shell).not.toHaveAttribute('data-sidebar', 'collapsed')

    await app.getByRole('button', { name: 'Toggle sidebar' }).click()
    await expect(shell).toHaveAttribute('data-sidebar', 'collapsed')

    await app.reload()
    await expect(app.locator('.app')).toHaveAttribute('data-sidebar', 'collapsed')

    await app.getByRole('button', { name: 'Toggle sidebar' }).click()
    await expect(app.locator('.app')).not.toHaveAttribute('data-sidebar', 'collapsed')
  })
})
