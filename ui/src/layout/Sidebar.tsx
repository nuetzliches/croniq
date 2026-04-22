import { NavLink } from 'react-router'
import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import * as Tooltip from '@radix-ui/react-tooltip'
import {
  LayoutDashboard, Briefcase, CalendarClock, Cpu, List, MailX,
  Settings, ChevronLeft, ChevronRight,
} from 'lucide-react'
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

const links = [
  { to: '/', label: 'Dashboard', icon: LayoutDashboard },
  { to: '/jobs', label: 'Jobs', icon: Briefcase },
  { to: '/schedules', label: 'Schedules', icon: CalendarClock },
  { to: '/runners', label: 'Runners', icon: Cpu },
  { to: '/executions', label: 'Executions', icon: List },
  { to: '/dead-letters', label: 'Dead Letters', icon: MailX },
  { to: '/calendars', label: 'Calendars', icon: CalendarClock },
  { to: '/settings', label: 'Settings', icon: Settings },
]

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
        {/* Logo */}
        <div className={cn('border-b border-border flex items-center gap-2 h-12', collapsed ? 'justify-center px-0' : 'px-4')}>
          <img src="/favicon.svg" alt="Croniq" className="h-6 w-6 shrink-0" />
          {!collapsed && <span className="font-semibold text-sm">Croniq</span>}
        </div>

        {/* Nav */}
        <nav className="flex-1 p-2 space-y-0.5" aria-label="Main navigation">
          {links.map((link) => {
            const Icon = link.icon
            const navItem = (
              <NavLink
                key={link.to}
                to={link.to}
                end={link.to === '/'}
                className={({ isActive }) =>
                  cn(
                    'flex items-center gap-2.5 rounded-md text-sm transition-colors',
                    collapsed ? 'justify-center px-0 py-2 w-10 h-10 mx-auto' : 'px-3 py-2',
                    isActive
                      ? 'bg-primary/10 text-primary font-medium'
                      : 'text-muted-foreground hover:bg-accent hover:text-accent-foreground'
                  )
                }
                aria-label={collapsed ? link.label : undefined}
              >
                <Icon className="h-4 w-4 shrink-0" />
                {!collapsed && <span>{link.label}</span>}
              </NavLink>
            )

            if (!collapsed) return navItem

            return (
              <Tooltip.Root key={link.to}>
                <Tooltip.Trigger asChild>{navItem}</Tooltip.Trigger>
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
          })}
        </nav>

        {/* Collapse toggle */}
        <div className="p-2 border-t border-border">
          <button
            onClick={toggle}
            aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
            aria-expanded={!collapsed}
            className={cn(
              'flex items-center gap-2 rounded-md text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors text-xs',
              collapsed ? 'justify-center w-10 h-10 mx-auto' : 'w-full px-3 py-2'
            )}
          >
            {collapsed ? <ChevronRight className="h-4 w-4" /> : (
              <>
                <ChevronLeft className="h-4 w-4" />
                <span>Collapse</span>
              </>
            )}
          </button>
        </div>
      </aside>
    </Tooltip.Provider>
  )
}
