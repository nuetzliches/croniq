/**
 * Which dashboard the suite is pointed at, and the handful of contracts the
 * two trees genuinely spell differently.
 *
 * ADR-0004 names this suite as the acceptance gate for the Vue rebuild, but
 * until now it only ever ran against the React build the server serves — so
 * the gate was measuring the tree being replaced (#620). It runs against both
 * now, one tree per invocation, chosen with `CRONIQ_E2E_TREE`.
 *
 * Everything the specs assert is deliberately framework-agnostic: routes,
 * session behaviour, URL contracts, the SSE surfaces. The differences below
 * are not implementation details leaking into tests — they are places where
 * the two dashboards make *different promises to a person*, and a link that
 * worked in one will not work in the other. Keeping them here, named and
 * counted, is the honest way to see how many there are.
 */
import type { Page } from '@playwright/test'

export type Tree = 'vue' | 'react'

export const TREE: Tree = (process.env.CRONIQ_E2E_TREE as Tree) ?? 'vue'

interface NavEntry {
  label: string
  path: string
}

interface TreeContract {
  /** Sidebar entries, by the label a person clicks. */
  nav: NavEntry[]
  /**
   * A settings sub-view, as a URL.
   *
   * React puts it in a query parameter (`?tab=clients`); the Vue tree uses a
   * path segment, matching how the rest of that tree addresses several views
   * of one screen. Both are real contracts; they are simply different ones.
   */
  settingsClients: string
  /** A settings URL naming a sub-view that does not exist. */
  settingsUnknown: string
  /** What must be visible once the unknown one falls back. */
  settingsFallback: RegExp
  /**
   * Open the account menu and click "Sign out".
   *
   * Both halves differ. The React shell has no accessible name on its trigger
   * — it is addressed by the `.user-pill` class, the line `auth.spec.ts` used
   * to flag as the one thing in this suite that was not framework-agnostic —
   * and its menu entries are plain buttons inside a `role="menu"` with no
   * `menuitem` role. The Vue shell names its trigger and renders real
   * `menuitem`s. Same promise, two spellings; the spec asserts the promise.
   */
  signOut: (page: Page) => Promise<void>
  /** Choose a colour theme, from wherever that tree puts the control. */
  pickTheme: (page: Page, theme: 'light' | 'dark') => Promise<void>
}

const CONTRACTS: Record<Tree, TreeContract> = {
  vue: {
    nav: [
      { label: 'Dashboard', path: '/' },
      { label: 'Runs', path: '/executions' },
      { label: 'Runners', path: '/runners' },
      { label: 'Dead Letters', path: '/dead-letters' },
      { label: 'Jobs', path: '/jobs' },
      { label: 'Calendars', path: '/calendars' },
      { label: 'Alerts', path: '/alerts' },
      { label: 'Settings', path: '/settings' },
    ],
    settingsClients: '/settings/clients',
    settingsUnknown: '/settings/not-a-section',
    settingsFallback: /not found/i,
    signOut: async (page) => {
      await page.getByRole('button', { name: /account menu/i }).click()
      await page.getByRole('menuitem', { name: 'Sign out' }).click()
    },
    // A separate topbar control in this tree, not an entry in the account
    // menu — it is a per-browser display preference rather than something
    // about the account, and it sits with the other display controls.
    pickTheme: async (page, theme) => {
      await page.getByRole('button', { name: /colour theme/i }).click()
      await page.getByRole('menuitemcheckbox', { name: new RegExp(theme, 'i') }).click()
    },
  },
  react: {
    nav: [
      { label: 'Dashboard', path: '/' },
      { label: 'Jobs', path: '/jobs' },
      { label: 'Executions', path: '/executions' },
      { label: 'Runners', path: '/runners' },
      { label: 'Dead Letters', path: '/dead-letters' },
      { label: 'Alerts', path: '/alerts' },
      { label: 'Calendars', path: '/calendars' },
      { label: 'Settings', path: '/settings' },
    ],
    settingsClients: '/settings?tab=clients',
    settingsUnknown: '/settings?tab=not-a-tab',
    settingsFallback: /profile/i,
    signOut: async (page) => {
      await page.locator('.user-pill').click()
      await page.locator('.user-menu').getByRole('button', { name: 'Sign out' }).click()
    },
    pickTheme: async (page, theme) => {
      await page.locator('.user-pill').click()
      await page.locator('.user-menu').getByRole('radio', { name: new RegExp(theme, 'i') }).click()
    },
  },
}

export const contract = CONTRACTS[TREE]
