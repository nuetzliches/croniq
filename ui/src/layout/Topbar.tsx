import { useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router'
import { PanelLeft, Search, Bell } from 'lucide-react'
import { useSidebarStore } from './sidebar-store'
import { useDeadLetters } from '@/api/hooks'
import { CommandPalette } from './CommandPalette'

const ROUTE_TITLES: Record<string, string> = {
  '/': 'Dashboard',
  '/jobs': 'Jobs',
  '/runners': 'Runners',
  '/executions': 'Executions',
  '/dead-letters': 'Dead Letters',
  '/calendars': 'Calendars',
  '/settings': 'Settings',
}

interface Crumb {
  label: string
  to?: string
  current?: boolean
}

function useCrumbs(): Crumb[] {
  const { pathname } = useLocation()

  if (pathname.startsWith('/jobs/')) {
    const jobKey = decodeURIComponent(pathname.slice('/jobs/'.length).split('/')[0] ?? '')
    return [
      { label: 'Jobs', to: '/jobs' },
      { label: jobKey || 'Job', current: true },
    ]
  }
  return [{ label: ROUTE_TITLES[pathname] ?? pathname, current: true }]
}

export function Topbar() {
  const toggle = useSidebarStore((s) => s.toggle)
  const navigate = useNavigate()
  const { data: deadLetters } = useDeadLetters()
  const dlCount = deadLetters?.length ?? 0
  const [paletteOpen, setPaletteOpen] = useState(false)

  const crumbs = useCrumbs()

  return (
    <header className="topbar" role="banner">
      <button
        type="button"
        className="topbar-toggle"
        onClick={toggle}
        aria-label="Toggle sidebar"
        title="Toggle sidebar"
      >
        <PanelLeft size={15} />
      </button>

      <nav className="crumbs" aria-label="Breadcrumb">
        <Link to="/" className="muted">
          Croniq
        </Link>
        {crumbs.map((c, i) => (
          <span key={`crumb-${i}`} style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <span className="sep">/</span>
            {c.to && !c.current ? (
              <Link to={c.to}>{c.label}</Link>
            ) : (
              <span className={c.current ? 'current' : ''}>{c.label}</span>
            )}
          </span>
        ))}
      </nav>

      <div className="topbar-search">
        <button
          type="button"
          className="search-trigger"
          onClick={() => setPaletteOpen(true)}
          aria-label="Open command palette"
        >
          <Search size={14} />
          <span className="grow">Search jobs, runners, executions…</span>
          <span className="kbd">⌘K</span>
        </button>
      </div>

      <div className="topbar-right">
        <span className="live" aria-hidden>
          <span className="live-dot" />
          <span>live</span>
        </span>
        <button
          type="button"
          className="btn icon ghost"
          onClick={() => navigate('/dead-letters')}
          aria-label={dlCount > 0 ? `${dlCount} dead letters pending` : 'Dead letters'}
          title="Dead letters"
          style={{ position: 'relative' }}
        >
          <Bell size={15} />
          {dlCount > 0 ? (
            <span
              aria-hidden
              style={{
                position: 'absolute',
                top: 6,
                right: 6,
                width: 7,
                height: 7,
                borderRadius: 999,
                background: 'var(--error)',
                boxShadow: '0 0 0 2px var(--bg)',
              }}
            />
          ) : null}
        </button>
      </div>

      {paletteOpen ? <CommandPalette onClose={() => setPaletteOpen(false)} /> : null}
    </header>
  )
}
