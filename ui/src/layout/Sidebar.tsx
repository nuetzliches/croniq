import { useState } from 'react'
import { NavLink, useMatch } from 'react-router'
import {
  LayoutDashboard,
  Briefcase,
  Cpu,
  List,
  MailX,
  CalendarDays,
  Settings,
  MoreHorizontal,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import clsx from 'clsx'
import { useDeadLetters, useCurrentUser } from '@/api/hooks'
import { useSidebarStore } from './sidebar-store'
import { Avatar, BrandMark } from '@/components/primitives'
import { UserMenu } from './UserMenu'

interface NavSpec {
  to: string
  label: string
  icon: LucideIcon
  badge?: { count: number; tone?: 'alert' | 'warn' | 'default' }
}

interface NavSection {
  kind: 'section'
  label: string
}

function navItems(deadLetters: number): (NavSpec | NavSection)[] {
  return [
    { to: '/', label: 'Dashboard', icon: LayoutDashboard },
    { to: '/jobs', label: 'Jobs', icon: Briefcase },
    { to: '/executions', label: 'Executions', icon: List },
    { to: '/runners', label: 'Runners', icon: Cpu },
    {
      to: '/dead-letters',
      label: 'Dead Letters',
      icon: MailX,
      badge:
        deadLetters > 0
          ? { count: deadLetters, tone: 'alert' as const }
          : undefined,
    },
    { kind: 'section', label: 'Config' },
    { to: '/calendars', label: 'Calendars', icon: CalendarDays },
    { to: '/settings', label: 'Settings', icon: Settings },
  ]
}

function NavRow({ spec, collapsed }: { spec: NavSpec; collapsed: boolean }) {
  const end = spec.to === '/'
  const match = useMatch({ path: spec.to, end })
  const isActive = match !== null
  const Icon = spec.icon
  return (
    <NavLink
      to={spec.to}
      end={end}
      className={clsx('nav-item', isActive && 'active')}
      title={collapsed ? spec.label : undefined}
      aria-label={collapsed ? spec.label : undefined}
    >
      <Icon className="icon" />
      <span>{spec.label}</span>
      {spec.badge ? (
        <span className={clsx('badge', spec.badge.tone === 'alert' && 'alert', spec.badge.tone === 'warn' && 'warn')}>
          {spec.badge.count > 99 ? '99+' : spec.badge.count}
        </span>
      ) : null}
    </NavLink>
  )
}

export function Sidebar() {
  const [userMenuOpen, setUserMenuOpen] = useState(false)
  const collapsedPref = useSidebarStore((s) => s.collapsed)
  const { data: deadLetters } = useDeadLetters()
  const { data: me } = useCurrentUser()

  // Collapsed state is purely user-driven via the persisted preference.
  // Below ~1023 px the page is still fully usable with the expanded
  // sidebar (232 px) — and users who want more room manually collapse
  // through the topbar toggle. The earlier auto-force-collapsed-below-
  // 1024 left the topbar toggle as a no-op, which the operator (rightly)
  // called confusing. Make it always work.
  const collapsed = collapsedPref

  const items = navItems(deadLetters?.length ?? 0)

  const displayName = me?.display_name || me?.username || 'Operator'
  const email = me?.email ?? me?.username ?? ''

  return (
    <aside className="sidebar" data-collapsed={collapsed ? '' : undefined}>
      <div className="brand">
        <span className="gear">
          <BrandMark size={18} />
        </span>
        <span className="name">Croniq</span>
      </div>

      <nav className="nav" aria-label="Main navigation">
        {items.map((it, i) => {
          if ('kind' in it) {
            return (
              <div key={`sec-${i}`} className="nav-section">
                {it.label}
              </div>
            )
          }
          return <NavRow key={it.to} spec={it} collapsed={collapsed} />
        })}
      </nav>

      <div className="sidebar-foot">
        <button
          type="button"
          className="user-pill"
          onClick={() => setUserMenuOpen((o) => !o)}
          aria-expanded={userMenuOpen}
          aria-haspopup="menu"
        >
          <Avatar name={displayName} />
          <div className="col">
            <span className="name ellipsis">{displayName}</span>
            {email ? <span className="email ellipsis">{email}</span> : null}
          </div>
          <MoreHorizontal size={14} className="more-icon" />
        </button>
        {userMenuOpen ? <UserMenu onClose={() => setUserMenuOpen(false)} /> : null}
      </div>
    </aside>
  )
}
