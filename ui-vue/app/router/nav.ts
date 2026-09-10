/**
 * The navigation, as decided in `docs/ui-screen-inventory.md`.
 *
 * Kept beside the route table rather than inside the shell component so the
 * two cannot disagree about which routes exist — a nav entry pointing at a
 * route nobody registered is a dead link that type checking will not catch.
 */
export interface NavItem {
  to: string
  label: string
  icon: string
  /** Hidden from known non-admins. Currently only the console. */
  adminOnly?: boolean
}

export interface NavSection {
  label: string
  items: NavItem[]
}

export const NAV_SECTIONS: NavSection[] = [
  {
    label: 'Operations',
    items: [
      { to: '/', label: 'Dashboard', icon: 'i-lucide-layout-dashboard' },
      { to: '/executions', label: 'Runs', icon: 'i-lucide-list' },
      { to: '/runners', label: 'Runners', icon: 'i-lucide-cpu' },
      { to: '/dead-letters', label: 'Dead Letters', icon: 'i-lucide-mail-x' },
    ],
  },
  {
    label: 'Configuration',
    items: [
      { to: '/jobs', label: 'Jobs', icon: 'i-lucide-briefcase' },
      { to: '/calendars', label: 'Calendars', icon: 'i-lucide-calendar-days' },
      { to: '/alerts', label: 'Alerts', icon: 'i-lucide-bell' },
    ],
  },
  {
    label: 'System',
    items: [
      { to: '/console', label: 'Console', icon: 'i-lucide-terminal', adminOnly: true },
      { to: '/settings', label: 'Settings', icon: 'i-lucide-settings' },
    ],
  },
]
