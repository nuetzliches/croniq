import { test, expect, login } from './fixtures'

/**
 * The dashboard's failure heatmap counts days and hours in the browser's zone,
 * the one every other time on the dashboard is shown in, and labels each row
 * with its own date (#864). It used to count in UTC and label the rows from a
 * fixed Mon–Sun list, so today read "Sun" whatever day it was.
 *
 * Kiritimati is UTC+14: for most of the UTC day its date is already
 * tomorrow's, so a heatmap that still counted in UTC would label its last row
 * with the wrong day. A context of its own, then, rather than the shared `app`
 * page, which is in the machine's zone.
 */
test.use({ timezoneId: 'Pacific/Kiritimati' })

test('the failure heatmap counts days in the browser zone', async ({ page }) => {
  await login(page)
  await page.goto('/')

  await expect(page.getByTestId('heatmap-zone')).toHaveText('· Pacific/Kiritimati')
  const days = page.getByTestId('heatmap-day')
  await expect(days).toHaveCount(7)
  const today = await page.evaluate(() =>
    new Intl.DateTimeFormat(undefined, { weekday: 'short' }).format(new Date()),
  )
  await expect(days.last()).toHaveText(today)
})
