import { useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router'
import { PanelLeft, Search, Bell } from 'lucide-react'
import { useSidebarStore } from './sidebar-store'
import { useCurrentUser, useDeadLetter, useDeadLetters, useVersion } from '@/api/hooks'
import { EnvBadge, VersionChip } from '@/components/primitives'
import { CommandPalette } from './CommandPalette'
import { MaintenanceControl } from './maintenance'
import { useRunnersStream } from '@/api/runners-stream'
import { isMac } from '@/lib/utils'
import clsx from 'clsx'

const ROUTE_TITLES: Record<string, string> = {
  '/': 'Dashboard',
  '/jobs': 'Jobs',
  '/runners': 'Runners',
  '/executions': 'Executions',
  '/dead-letters': 'Dead Letters',
  '/alerts': 'Alerts',
  '/calendars': 'Calendars',
  '/console': 'Console',
  '/settings': 'Settings',
}

// Fallback label for any top-level route missing from ROUTE_TITLES. Without
// it the raw pathname ("/alerts") was used verbatim — and since every crumb
// renders after a "/" separator, that leading slash showed up as a doubled
// "//". Strip the slash and Title-Case the first segment so a forgotten
// ROUTE_TITLES entry degrades to "Alerts" instead of "/alerts".
function titleFromPath(pathname: string): string {
  const seg = pathname.replace(/^\/+/, '').split('/')[0] ?? ''
  if (!seg) return 'Dashboard'
  return seg
    .split('-')
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(' ')
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
  if (pathname.startsWith('/dead-letters/')) {
    const id = pathname.slice('/dead-letters/'.length)
    return [
      { label: 'Dead Letters', to: '/dead-letters' },
      { label: id, current: true, _dlId: id } as Crumb & { _dlId: string },
    ]
  }
  if (pathname.startsWith('/executions/')) {
    // Executions are UUIDs — show the leading 8 chars so the crumb stays
    // scannable instead of dumping a 36-char hex blob into the topbar.
    const id = pathname.slice('/executions/'.length)
    return [
      { label: 'Executions', to: '/executions' },
      { label: `${id.slice(0, 8)}…`, current: true },
    ]
  }
  if (pathname.startsWith('/runners/')) {
    const id = decodeURIComponent(pathname.slice('/runners/'.length))
    return [
      { label: 'Runners', to: '/runners' },
      { label: id, current: true },
    ]
  }
  return [{ label: ROUTE_TITLES[pathname] ?? titleFromPath(pathname), current: true }]
}

// Resolve dead-letter id to its job_key so the breadcrumb reads as a name
// instead of a UUID. Falls back to the short id while loading or if the
// entry has already been replayed/deleted (404).
function DLBreadcrumbLabel({ id }: { id: string }) {
  const { data } = useDeadLetter(id)
  const label = data?.job_key ?? `${id.slice(0, 8)}…`
  return <>{label}</>
}

export function Topbar() {
  const toggle = useSidebarStore((s) => s.toggle)
  const navigate = useNavigate()
  const { data: deadLetters } = useDeadLetters()
  const { data: version } = useVersion()
  const { data: me } = useCurrentUser()
  const dlCount = deadLetters?.length ?? 0
  const [paletteOpen, setPaletteOpen] = useState(false)
  const { isConnected } = useRunnersStream()

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
        {version ? <EnvBadge env={version.env} /> : null}
        {crumbs.map((c, i) => {
          const dlId = (c as Crumb & { _dlId?: string })._dlId
          return (
          <span key={`crumb-${i}`} style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <span className="sep">/</span>
            {c.to && !c.current ? (
              <Link to={c.to}>{c.label}</Link>
            ) : dlId ? (
              <span className={c.current ? 'current' : ''}>
                <DLBreadcrumbLabel id={dlId} />
              </span>
            ) : (
              <span className={c.current ? 'current' : ''}>{c.label}</span>
            )}
          </span>
          )
        })}
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
          <span className="kbd">{isMac ? '⌘K' : 'Ctrl K'}</span>
        </button>
      </div>

      <div className="topbar-right">
        {version ? <VersionChip version={version} /> : null}
        {me?.role === 'admin' ? <MaintenanceControl /> : null}
        <span
          className={clsx('live', !isConnected && 'offline')}
          role="status"
          title={isConnected ? 'Live — receiving realtime updates' : 'Disconnected — reconnecting…'}
        >
          <span className="live-dot" />
          <span>{isConnected ? 'live' : 'offline'}</span>
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
