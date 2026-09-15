import { test, expect } from './fixtures'

/**
 * Names that point at something, pointing at it.
 *
 * Three places named a job or a runner in the accent colour and did nothing
 * when clicked: the dashboard's "what fires next" rail, and the Job and Runner
 * rows of a run's detail. `tsc` cannot see the difference between a coloured
 * span and a link, and neither can a reader until they click — which is why
 * these sat there.
 *
 * Asserted through the link role rather than by looking for an `<a>`: what
 * matters is that it is reachable, announced and followable, not which element
 * it happens to be.
 */
/*
 * The dashboard's next-fire rail is the third place, and it is not here: the
 * e2e stack's demo jobs have no armed trigger, so the rail is permanently on
 * its "Nothing scheduled" branch and there is nothing to click. Faking the
 * states response would test the fake. Left to review and to the screenshot in
 * the PR rather than pinned by a test that could only assert a stub.
 */
test.describe('entity links', () => {
  test('a run detail links to its job and to that runner’s runs', async ({ app }) => {
    // The suite's own run, rather than waiting for the demo schedule: the
    // fastest demo job fires once a minute, which is longer than this test's
    // budget and would make it a clock test rather than a link test.
    await app.goto('/jobs')
    await app.locator('tbody tr').first().click()
    await app.getByRole('button', { name: 'Run now' }).click()

    await app.goto('/executions')
    const detail = app.getByRole('complementary', { name: 'Run detail' })

    // Poll for a run that has been claimed: `runner_id` is null until a runner
    // takes it, and the Runner row is the point of half this test.
    await expect
      .poll(
        async () => {
          await app.reload()
          const row = app.locator('tbody tr').first()
          if (!(await row.count())) return false
          await row.click()
          return detail.getByRole('link').count().then((n) => n >= 2)
        },
        { message: 'waiting for a claimed run', timeout: 20_000 },
      )
      .toBe(true)

    const runnerLink = detail.getByRole('link').nth(1)
    const runnerId = (await runnerLink.innerText()).trim()

    await detail.getByRole('link').first().click()
    await expect(app).toHaveURL(/\/jobs\//)

    await app.goBack()
    await app.locator('tbody tr').first().click()
    await detail.getByRole('link').nth(1).click()

    // A runner has no detail page — `/runners/:runnerId` redirects to the
    // fleet — so its name links to its runs, the same target the fleet row
    // uses. The filter carrying the id through is what makes that a link to
    // *this* runner rather than to the runs screen.
    await expect(app).toHaveURL(new RegExp(`runner_id=${encodeURIComponent(runnerId)}`))
  })

  test('a healthy runner is coloured as healthy, not as neutral', async ({ app }) => {
    await app.goto('/runners')

    // The dot is this repo's own markup (`StatusPill.vue`), not the badge
    // library's, so it says what the state was mapped to rather than how Nuxt
    // UI spells a colour this week. `online` was absent from that mapping and
    // fell through to the neutral default, so a healthy runner rendered in the
    // same grey as a disabled job.
    const pill = app.locator('tbody tr').first().getByText('online', { exact: true })
    await expect(pill).toBeVisible()
    await expect(pill.locator('span[aria-hidden="true"]')).toHaveClass(/bg-success/)
  })
})
