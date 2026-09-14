import { test, expect } from './fixtures'

/**
 * Creating, editing and deleting — the paths that change something.
 *
 * The suite had none of these. `auth`, `navigation`, `url-state` and `live`
 * all read; the only coverage of a write was `scripts/write-paths.mjs`, a
 * 1065-line walker that logs `FAIL` and exits 0, and which nothing in CI runs
 * (issue #677). So the four bugs found in this batch — a schedule window
 * silently dropped, a confirmation that was not there, a mutation that failed
 * in silence, a force-replay with no route to it — were all in code no test
 * would have exercised.
 *
 * These specs assert the *contract* rather than the layout: something was
 * created, the list shows it, deleting asks first, and a refusal is visible.
 * A spec that pins the arrangement of a screen becomes a reason not to improve
 * the screen.
 *
 * Every job key is unique per run. The stack is fresh per run in CI but not on
 * a laptop, and a spec that only passes on a clean database is a spec people
 * learn to ignore.
 */

/** A key nothing else in this run will collide with. */
function uniqueKey(prefix: string): string {
  return `e2e:${prefix}-${Date.now().toString(36)}-${Math.floor(Math.random() * 1e4)}`
}

test.describe('job lifecycle', () => {
  test('a job can be created, edited and deleted', async ({ app }) => {
    const key = uniqueKey('lifecycle')

    // ── create ──────────────────────────────────────────────────────────────
    await app.goto('/jobs')
    await app.getByRole('button', { name: 'New job' }).first().click()
    await app.getByLabel('Job key', { exact: true }).fill(key)
    await app.getByLabel('Description').fill('Created by the e2e suite')

    const [created] = await Promise.all([
      app.waitForResponse((r) => r.url().includes('/v1/jobs') && r.request().method() === 'POST'),
      app.getByRole('button', { name: 'Create job' }).click(),
    ])
    expect(created.status(), 'creating a job must succeed').toBe(201)

    // The list is the contract, not the form: a create that the list does not
    // show is a create the operator cannot find again.
    await app.goto('/jobs')
    await expect(app.getByText(key, { exact: true }).first()).toBeVisible()

    // ── edit ────────────────────────────────────────────────────────────────
    await app.getByText(key, { exact: true }).first().click()
    await app.getByRole('button', { name: 'Edit' }).click()
    await app.getByLabel('Timeout').fill('90s')

    const [saved] = await Promise.all([
      app.waitForResponse((r) => r.url().includes('/v1/jobs/') && r.request().method() === 'PUT'),
      app.getByRole('button', { name: /^save$/i }).click(),
    ])
    expect(saved.status(), 'editing a job must succeed').toBe(200)

    // ── delete, which must ask ──────────────────────────────────────────────
    await app.getByRole('button', { name: `Delete ${key}` }).click()

    // The dialog is the point of this half. Before #667 several destructive
    // controls fired straight from the click.
    const dialog = app.getByRole('dialog')
    await expect(dialog).toBeVisible()
    await expect(dialog).toContainText(key)

    const [deleted] = await Promise.all([
      app.waitForResponse(
        (r) => r.url().includes('/v1/jobs/') && r.request().method() === 'DELETE',
      ),
      dialog.getByRole('button', { name: /^delete$/i }).click(),
    ])
    expect(deleted.status(), 'deleting a job must succeed').toBe(204)

    await app.goto('/jobs')
    await expect(app.getByText(key, { exact: true })).toHaveCount(0)
  })

  test('cancelling a delete leaves the job alone', async ({ app }) => {
    // The other half of a confirmation: it has to be refusable. A dialog whose
    // Cancel deletes anyway is worse than no dialog.
    const key = uniqueKey('kept')

    await app.goto('/jobs')
    await app.getByRole('button', { name: 'New job' }).first().click()
    await app.getByLabel('Job key', { exact: true }).fill(key)
    await Promise.all([
      app.waitForResponse((r) => r.url().includes('/v1/jobs') && r.request().method() === 'POST'),
      app.getByRole('button', { name: 'Create job' }).click(),
    ])

    await app.goto('/jobs')
    await app.getByText(key, { exact: true }).first().click()
    await app.getByRole('button', { name: `Delete ${key}` }).click()

    const dialog = app.getByRole('dialog')
    await expect(dialog).toBeVisible()
    await dialog.getByRole('button', { name: /cancel/i }).click()
    await expect(dialog).toBeHidden()

    await app.goto('/jobs')
    await expect(app.getByText(key, { exact: true }).first()).toBeVisible()
  })
})

test.describe('the server refusing something', () => {
  test('a rejected create says why, in the server’s words', async ({ app }) => {
    // The failure mode this covers is silence. A form that reports nothing on
    // a 4xx looks exactly like one that worked (issue #664), and the server's
    // own message is more useful than anything the form could invent.
    //
    // A malformed duration rather than a malformed key: the server parses
    // `timeout` and answers `400 invalid_duration`, while a key without a
    // namespace separator is accepted. Picking a refusal the server actually
    // makes is the difference between testing the form and testing an
    // assumption about the server.
    const key = uniqueKey('refused')

    await app.goto('/jobs')
    await app.getByRole('button', { name: 'New job' }).first().click()
    await app.getByLabel('Job key', { exact: true }).fill(key)
    await app.getByLabel('Timeout').fill('5min')

    const [refused] = await Promise.all([
      app.waitForResponse(
        (r) => r.url().includes('/v1/jobs') && r.request().method() === 'POST',
      ),
      app.getByRole('button', { name: 'Create job' }).click(),
    ])
    expect(refused.status(), 'a malformed duration must be refused').toBe(400)

    // Visible, and carrying the server's own wording rather than a generic
    // apology.
    const alert = app.getByRole('alert')
    await expect(alert).toBeVisible()
    await expect(alert).toContainText(/duration|timeout|5min/i)

    // And nothing was created.
    await app.goto('/jobs')
    await expect(app.getByText(key, { exact: true })).toHaveCount(0)
  })
})
