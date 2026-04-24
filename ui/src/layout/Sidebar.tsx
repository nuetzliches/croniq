import { NavLink, useMatch } from 'react-router'
import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import * as Tooltip from '@radix-ui/react-tooltip'
import {
  LayoutDashboard, Briefcase, CalendarClock, CalendarDays, Cpu, List, MailX,
  Settings, PanelLeftClose, PanelLeftOpen,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import { cn } from '@/lib/utils'

interface SidebarStore {
  collapsed: boolean
  toggle: () => void
}

export const useSidebarStore = create<SidebarStore>()(
  persist(
    (set) => ({
      collapsed: false,
      toggle: () => set((s) => ({ collapsed: !s.collapsed })),
    }),
    { name: 'croniq_sidebar' }
  )
)

interface NavLinkSpec {
  to: string
  label: string
  icon: LucideIcon
}

const links: NavLinkSpec[] = [
  { to: '/', label: 'Dashboard', icon: LayoutDashboard },
  { to: '/jobs', label: 'Jobs', icon: Briefcase },
  { to: '/schedules', label: 'Schedules', icon: CalendarClock },
  { to: '/runners', label: 'Runners', icon: Cpu },
  { to: '/executions', label: 'Executions', icon: List },
  { to: '/dead-letters', label: 'Dead Letters', icon: MailX },
  { to: '/calendars', label: 'Calendars', icon: CalendarDays },
  { to: '/settings', label: 'Settings', icon: Settings },
]

// The nav item pre-computes `isActive` and passes a plain string className.
// NavLink's function-className form gets stringified by `<Tooltip.Trigger
// asChild>` (Radix Slot merges props by joining classNames as strings),
// which collapses the callback's source into the DOM class attribute —
// causing every item to render with a bare `text-primary` token and broken
// padding tokens like `p-2.5:px-3`.
function NavItem({ link, collapsed }: { link: NavLinkSpec; collapsed: boolean }) {
  const end = link.to === '/'
  const match = useMatch({ path: link.to, end })
  const isActive = match !== null
  const Icon = link.icon

  // Same padding in both states keeps the icon at a fixed x-offset.
  // If we switched to `justify-center p-2.5` while the sidebar width is
  // still mid-transition, every icon would snap to the middle of the
  // still-wide sidebar and slide back as it narrows — visible as a jump.
  // `h-9` locks the item height across states: expanded items naturally
  // compute to 36px (text line-height 20 + 16 padding), collapsed items
  // would otherwise shrink to 32px because the hidden span removes the
  // line-height contribution, making the nav total height flicker on
  // collapse.
  // `whitespace-nowrap overflow-hidden` keeps two-word labels (Dead
  // Letters) from wrapping to a second line during early expand frames
  // when the item is still narrow. Without it the span breaks at the
  // space, jumps to 40px tall, and `items-center` re-seats the icon —
  // visible as a Dead-Letters-only layout twitch.
  const className = cn(
    'flex items-center gap-2.5 rounded-md text-sm transition-colors mx-2 px-3 h-9',
    'overflow-hidden whitespace-nowrap',
    isActive
      ? 'bg-primary/10 text-primary font-medium'
      : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'
  )

  const navLink = (
    <NavLink
      to={link.to}
      end={end}
      className={className}
      aria-label={collapsed ? link.label : undefined}
    >
      <Icon className="h-4 w-4 shrink-0" />
      {!collapsed && <span>{link.label}</span>}
    </NavLink>
  )

  if (!collapsed) return navLink

  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>{navLink}</Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content
          side="right"
          className="z-50 rounded-md bg-foreground px-2.5 py-1 text-xs text-background shadow-md"
        >
          {link.label}
          <Tooltip.Arrow className="fill-foreground" />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}

export function Sidebar() {
  const { collapsed, toggle } = useSidebarStore()

  return (
    <Tooltip.Provider delayDuration={0}>
      <aside
        className={cn(
          'border-r border-border bg-card flex flex-col transition-all duration-200 shrink-0',
          collapsed ? 'w-14' : 'w-56'
        )}
        aria-label="Main navigation"
      >
        {/* Logo — constant left padding so the icon stays at the same
            x-offset while the sidebar animates its width. No right padding
            so the 24px logo never gets subpixel-squeezed against the edge
            when collapsed (aside=56px would leave exactly 0px of slack). */}
        <div className="border-b border-border flex items-center gap-2 h-12 pl-4">
          <img src="/favicon.svg" alt="Croniq" className="h-6 w-6 shrink-0" />
          {!collapsed && <span className="font-semibold text-sm">Croniq</span>}
        </div>

        {/* Nav */}
        <nav className="flex-1 py-2 space-y-0.5" aria-label="Main navigation">
          {links.map((link) => (
            <NavItem key={link.to} link={link} collapsed={collapsed} />
          ))}
        </nav>

        {/* Collapse toggle — right-aligned in both states so it smoothly
            slides to the corner as the sidebar narrows instead of snapping
            to center mid-animation. */}
        <div className="flex justify-end px-2 pb-2">
          <Tooltip.Root>
            <Tooltip.Trigger asChild>
              <button
                onClick={toggle}
                aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
                aria-expanded={!collapsed}
                className="p-2 rounded-md text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors"
              >
                {collapsed
                  ? <PanelLeftOpen className="h-4 w-4" />
                  : <PanelLeftClose className="h-4 w-4" />
                }
              </button>
            </Tooltip.Trigger>
            <Tooltip.Portal>
              <Tooltip.Content
                side="right"
                className="z-50 rounded-md bg-foreground px-2.5 py-1 text-xs text-background shadow-md"
              >
                {collapsed ? 'Expand' : 'Collapse'}
                <Tooltip.Arrow className="fill-foreground" />
              </Tooltip.Content>
            </Tooltip.Portal>
          </Tooltip.Root>
        </div>
      </aside>
    </Tooltip.Provider>
  )
}
