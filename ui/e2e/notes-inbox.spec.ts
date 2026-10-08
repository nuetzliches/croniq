import type { APIRequestContext } from '@playwright/test'
import { API_KEY, expect, test } from './fixtures'

/**
 * The notes inbox, against a real server and a second person.
 *
 * The inbox only means something with two people in it: "unread" is written
 * by someone else, and the badge never counts what the reader wrote. So this
 * spec makes a colleague through the API — a real user with a real login —
 * who writes while the suite's admin reads.
 *
 * The colleague signs in through a request context of their own, never
 * through the shared page: a login there would replace the admin's refresh
 * cookie, and every spec after this one would be running as someone else.
 */

const COLLEAGUE_PASSWORD = 'colleague-e2e-password'

/** The seeded API key: sets the colleague up, and clears up after. */
let admin: APIRequestContext
/** The colleague, signed in. */
let colleague: APIRequestContext
let colleagueId = ''
/** Every note this spec wrote, so none outlives it. */
const written: string[] = []

/** A note body nothing else in the run will have written. */
function uniqueBody(text: string): string {
  return `${text} (${Date.now().toString(36)}-${Math.floor(Math.random() * 1e4)})`
}

test.describe('notes inbox', () => {
  test.beforeAll(async ({ playwright }, testInfo) => {
    const baseURL = testInfo.project.use.baseURL
    admin = await playwright.request.newContext({
      baseURL,
      extraHTTPHeaders: { Authorization: `ApiKey ${API_KEY}` },
    })
    const username = `colleague-${Date.now().toString(36)}`
    const created = await admin.post('/v1/users', {
      data: { username, password: COLLEAGUE_PASSWORD, role: 'operator', display_name: 'Colleague' },
    })
    expect(created.status(), await created.text()).toBe(201)
    colleagueId = (await created.json()).user_id

    const signIn = await playwright.request.newContext({ baseURL })
    const login = await signIn.post('/v1/auth/login', {
      data: { username, password: COLLEAGUE_PASSWORD },
    })
    expect(login.status(), await login.text()).toBe(200)
    const { access_token: token } = await login.json()
    await signIn.dispose()
    colleague = await playwright.request.newContext({
      baseURL,
      extraHTTPHeaders: { Authorization: `Bearer ${token}` },
    })
  })

  test.afterAll(async () => {
    for (const id of written) await admin.delete(`/v1/notes/${id}`)
    if (colleagueId) await admin.delete(`/v1/users/${colleagueId}`)
    await colleague?.dispose()
    await admin?.dispose()
  })

  async function colleagueAsks(body: string) {
    const response = await colleague.post('/v1/notes', {
      data: { job_key: 'demo:heartbeat', kind: 'question', body },
    })
    expect(response.status(), await response.text()).toBe(201)
    written.push((await response.json()).id)
  }

  test("a colleague's note is unread until the inbox is opened", async ({ app }) => {
    const body = uniqueBody('Why did the heartbeat skip a beat?')
    await colleagueAsks(body)

    const notesLink = app
      .getByRole('navigation', { name: 'Main navigation' })
      .getByRole('link', { name: /^Notes/ })
    await app.goto('/jobs')
    await expect(notesLink.getByTestId('nav-badge')).toBeVisible()

    await notesLink.click()
    await expect.poll(() => new URL(app.url()).pathname).toBe('/notes')
    // While the inbox is open only its own answer moves the badge — so this
    // is the page having marked what it showed, not a poll.
    await expect(notesLink.getByTestId('nav-badge')).toHaveCount(0)

    // …and what was new on arrival still says so, for the rest of the visit.
    const row = app.locator('tr', { hasText: body })
    await expect(row).toHaveAttribute('data-unread', 'true')
    await app.getByRole('radio', { name: 'Unread', exact: true }).click()
    await expect(app).toHaveURL(/show=unread/)
    await expect(row).toBeVisible()

    // The next visit starts where this one left off: nothing is new any more.
    await app.reload()
    await expect(app.getByText('All caught up')).toBeVisible()
    await app.getByRole('radio', { name: 'All', exact: true }).click()
    await expect(row).toBeVisible()
    await expect(row).not.toHaveAttribute('data-unread', 'true')
  })

  test('Mine lists the threads I have written in', async ({ app }) => {
    // A job's own thread opens from its address alone, before any row does.
    await app.goto('/notes/jobs/demo%3Areport')
    const detail = app.getByRole('complementary', { name: 'Thread detail' })
    await expect(detail).toBeVisible()

    const body = uniqueBody('The report numbers look off by one.')
    await detail.getByLabel('Note text').fill(body)
    const [response] = await Promise.all([
      app.waitForResponse(
        (r) => new URL(r.url()).pathname === '/v1/notes' && r.request().method() === 'POST',
      ),
      detail.getByRole('button', { name: 'Add note' }).click(),
    ])
    expect(response.status()).toBe(201)
    written.push((await response.json()).id)

    await app.getByRole('radio', { name: 'Mine', exact: true }).click()
    await expect(app).toHaveURL(/show=mine/)
    await expect(app.locator('tr', { hasText: body })).toBeVisible()
  })

  test('g then o opens the inbox', async ({ app }) => {
    await app.goto('/jobs')
    await app.keyboard.press('g')
    await app.keyboard.press('o')
    await expect.poll(() => new URL(app.url()).pathname).toBe('/notes')
  })
})
