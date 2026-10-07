import { test, expect } from './fixtures'
import { pickTheme } from './app'

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

    // Both shells print "<filtered> / <total>" for the buffer — React
    // appends "events", the Vue toolbar does not, so the match stops at the
    // pair of numbers that is the actual assertion.
    await expect
      .poll(
        async () => {
          const text = (await app.getByText(/\d+\s*\/\s*\d+/).first().textContent()) ?? ''
          return Number(text.match(/\/\s*(\d+)/)?.[1] ?? 0)
        },
        { timeout: 20_000 },
      )
      .toBeGreaterThan(0)
  })

  /**
   * The startup summary is what an operator checks after a deploy, and the
   * tail cannot be trusted to still hold it (#810). The server pins it and
   * replays it on every connect; the console shows it as its own section.
   */
  test('the console shows the startup log with the configuration summary', async ({ app }) => {
    await app.goto('/console')
    const section = app.getByRole('button', { name: /^Startup/ })
    await expect(section).toBeVisible({ timeout: 20_000 })
    await expect(section).toHaveAttribute('aria-expanded', 'true')
    await expect(app.locator('#console-startup').getByText('configuration loaded').first()).toBeVisible()
    await expect(app.locator('#console-startup').getByText('failure-alert evaluator armed').first()).toBeVisible()

    await section.click()
    await expect(app.locator('#console-startup')).toHaveCount(0)
  })

  /**
   * The dashboard timeline holds still on request (#829): the Pause button,
   * or moving the range off "now", which the keyboard does in 5 s steps.
   * "Live" puts both back.
   */
  test('the live timeline pauses, looks back, and returns to live', async ({ app }) => {
    await app.goto('/')
    const state = app.getByTestId('live-state')
    await expect(state).toHaveText(/live/i, { timeout: 20_000 })
    // The strip reaches past "now" into the forecast: the demo jobs fire
    // within its five minutes, so there is something to the right of the line.
    await expect(app.getByTestId('live-range-now')).toBeVisible()
    await expect(app.getByTestId('live-range-forecast').first()).toBeVisible({ timeout: 20_000 })

    await app.getByTestId('live-pause').click()
    await expect(state).toHaveText(/paused/i)
    await app.getByTestId('live-resume').click()
    await expect(state).toHaveText(/live/i)

    // Pulling the range's end before "now" is a look back, and pauses.
    await app.getByTestId('live-range-end').focus()
    await app.keyboard.press('Shift+ArrowLeft')
    await expect(state).toHaveText(/paused/i)
    await app.getByTestId('live-resume').click()
    await expect(state).toHaveText(/live/i)
  })

  test('hovering the track holds it only while the hand is on', async ({ app }) => {
    await app.goto('/')
    const state = app.getByTestId('live-state')
    await expect(state).toHaveText(/live/i, { timeout: 20_000 })
    const track = app.getByTestId('live-track')
    const hand = app.getByTestId('live-hold-on-hover')
    await expect(hand).toHaveAttribute('aria-pressed', 'true')

    await track.hover()
    await expect(state).toHaveText('Live · held')
    await app.mouse.move(0, 0)
    await expect(state).toHaveText('Live')

    await hand.click()
    await expect(hand).toHaveAttribute('aria-pressed', 'false')
    await track.hover()
    await expect(state).toHaveText('Live')

    // The choice outlives a reload.
    await app.reload()
    await expect(app.getByTestId('live-hold-on-hover')).toHaveAttribute('aria-pressed', 'false')
  })
})

test.describe('live timeline links', () => {
  /**
   * Lane labels and bars are plain anchors behind one delegated handler
   * (they were `RouterLink`s, too heavy by the hundred). A click must still
   * navigate inside the app, not reload the page.
   */
  test('a lane label opens its job without a page load', async ({ app }) => {
    // A run of our own gives the card a lane now, rather than waiting up to a
    // minute for the demo schedule (the pattern entity-links.spec.ts uses).
    await app.goto('/jobs')
    await app.locator('tbody tr').first().click()
    await app.getByRole('button', { name: 'Run now' }).click()

    await app.goto('/')
    const label = app.getByTestId('live-lanes').locator('a[data-route^="/jobs/"]').first()
    await expect(label).toBeVisible({ timeout: 20_000 })
    const route = await label.getAttribute('data-route')
    await app.evaluate(() => ((window as unknown as { __sameDocument: boolean }).__sameDocument = true))
    await label.click()
    await expect.poll(() => new URL(app.url()).pathname).toBe(route)
    expect(await app.evaluate(() => (window as unknown as { __sameDocument?: boolean }).__sameDocument)).toBe(true)
  })
})

test.describe('preferences survive a reload', () => {
  test('the live timeline keeps its lane order and window width', async ({ app }) => {
    await app.goto('/')
    const order = app.getByTestId('live-order')
    await expect(order).toHaveText(/name/i, { timeout: 20_000 })
    await order.click()
    await expect(order).toHaveText(/next fire/i)

    // Widen by one keyboard step on the left edge: 1m → 1m 5s back.
    await app.getByTestId('live-range-start').focus()
    await app.keyboard.press('ArrowLeft')
    await expect(app.getByText('1m 5s back · 10s ahead')).toBeVisible()

    await app.reload()
    await expect(app.getByTestId('live-order')).toHaveText(/next fire/i, { timeout: 20_000 })
    await expect(app.getByText('1m 5s back · 10s ahead')).toBeVisible()

    // Restore the defaults for later tests.
    await app.getByTestId('live-order').click()
    await app.getByTestId('live-range-start').focus()
    await app.keyboard.press('ArrowRight')
  })

  test('the theme choice persists', async ({ app }) => {
    await pickTheme(app, 'light')
    await expect(app.locator('html')).toHaveAttribute('data-theme', 'light')

    await app.reload()
    await expect(app.locator('html')).toHaveAttribute('data-theme', 'light')

    // Restore, so a later test does not inherit a non-default theme.
    await pickTheme(app, 'dark')
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
