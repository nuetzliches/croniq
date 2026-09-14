import type { Page } from '@playwright/test'

/**
 * The dashboard's shape, where a spec needs to name it.
 *
 * This file replaces `trees.ts`, which existed to hold the handful of places
 * two dashboards promised different things while both existed. The React tree
 * is gone, so the divergence is gone with it and what is left is simply *this*
 * dashboard's nav and two interactions that are more than one selector each.
 *
 * Kept out of the specs so the specs read as claims about behaviour rather
 * than as click sequences.
 */
export const NAV: { label: string; path: string }[] = [
  { label: 'Dashboard', path: '/' },
  { label: 'Runs', path: '/executions' },
  { label: 'Runners', path: '/runners' },
  { label: 'Dead Letters', path: '/dead-letters' },
  { label: 'Jobs', path: '/jobs' },
  { label: 'Calendars', path: '/calendars' },
  { label: 'Alerts', path: '/alerts' },
  { label: 'Settings', path: '/settings' },
]

/** Open the account menu and sign out. */
export async function signOut(page: Page): Promise<void> {
  await page.getByRole('button', { name: /account menu/i }).click()
  await page.getByRole('menuitem', { name: 'Sign out' }).click()
}

/**
 * Choose a colour theme.
 *
 * A topbar control of its own rather than an entry in the account menu: it is
 * a per-browser display preference, not something about the account.
 */
export async function pickTheme(page: Page, theme: 'light' | 'dark'): Promise<void> {
  await page.getByRole('button', { name: /colour theme/i }).click()
  await page.getByRole('menuitemcheckbox', { name: new RegExp(theme, 'i') }).click()
}
