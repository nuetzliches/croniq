import { test, expect, login } from './fixtures'
import { NAV } from './app'

/**
 * Nothing the dashboard loads comes from another origin (ADR-0005).
 *
 * `scripts/check-local-sources.mjs` greps the built output for absolute URLs,
 * which catches the common case — a dependency with a CDN default baked into
 * its source. It cannot catch a URL assembled at runtime, which is precisely
 * how the failure that produced ADR-0005 worked: `api.iconify.design` was a
 * host in a resource list, joined to a query string built from icon names, and
 * no single literal in the bundle looked like a request.
 *
 * So this watches the wire instead. It is the check that matches the promise
 * as an operator would state it: open the dashboard on an air-gapped host and
 * nothing hangs.
 *
 * Its own context, not the shared `app` page: half of what a browser fetches
 * it fetches once, on the cold load, and a page that is already signed in has
 * done that long ago.
 */
test.describe('local sources', () => {
  test('no request leaves the dashboard origin', async ({ browser, baseURL }) => {
    const origin = new URL(baseURL!).origin
    const foreign: string[] = []

    const context = await browser.newContext()
    const page = await context.newPage()

    // `request`, not `response`: a request to a host that does not resolve —
    // the air-gapped case — never produces a response to observe, and that is
    // the case this test exists for.
    page.on('request', (request) => {
      const url = request.url()
      // `data:` and `blob:` are the page's own bytes under another scheme;
      // icons rendered in CSS mask mode arrive as `data:` URIs by design.
      if (!url.startsWith('http://') && !url.startsWith('https://')) return
      if (new URL(url).origin === origin) return
      foreign.push(`${request.resourceType()} ${url}`)
    })

    try {
      await login(page)

      // Every page, because icons are per-view: the sidebar's come from the
      // nav table, and each view names its own. A walk of one page would have
      // passed while the dashboard as a whole was fetching seventy icons.
      for (const { label, path } of NAV) {
        await page.getByRole('navigation', { name: 'Main navigation' }).getByRole('link', { name: label }).click()
        await expect.poll(() => new URL(page.url()).pathname).toBe(path)
      }

      // Icons resolve asynchronously when they resolve over the network, so a
      // walk that ends the instant the last route settles can outrun the very
      // requests it is looking for.
      await page.waitForLoadState('networkidle')

      expect(foreign, `requests to origins other than ${origin}`).toEqual([])
    } finally {
      await context.close()
    }
  })

  /**
   * The symptom the bug actually presented as. An icon that fails to load
   * leaves the `<span>` Iconify renders empty, and no amount of route
   * assertions notices — so assert on the pixels' proxy: the sidebar's icons
   * are `<svg>` elements, or they are nothing.
   */
  test('the sidebar icons render from the bundle', async ({ app }) => {
    const nav = app.getByRole('navigation', { name: 'Main navigation' })
    for (const { label } of NAV) {
      // `.first()`: the assertion is that the icon rendered at all, and a link
      // that grows a second glyph should not turn this into a strict-mode
      // failure about selectors.
      await expect(nav.getByRole('link', { name: label }).locator('svg').first()).toBeVisible()
    }
  })
})
