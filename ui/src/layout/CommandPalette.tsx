import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import {
  LayoutDashboard,
  Briefcase,
  Cpu,
  List,
  MailX,
  CalendarDays,
  Settings,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import clsx from 'clsx'
import { useJobs, useRunners } from '@/api/hooks'

interface PaletteItem {
  id: string
  icon: LucideIcon
  label: string
  sub?: string
  section: 'Actions' | 'Jobs' | 'Runners'
  go: () => void
  key?: string
}

export interface CommandPaletteProps {
  onClose: () => void
}

export function CommandPalette({ onClose }: CommandPaletteProps) {
  const navigate = useNavigate()
  const [q, setQ] = useState('')
  const [active, setActive] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const { data: jobs } = useJobs()
  const { data: runners } = useRunners()

  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  const items = useMemo<PaletteItem[]>(() => {
    const ql = q.trim().toLowerCase()
    const actions: PaletteItem[] = [
      { id: 'go-dashboard', section: 'Actions', icon: LayoutDashboard, label: 'Go to Dashboard',  key: 'G D', go: () => navigate('/') },
      { id: 'go-jobs',      section: 'Actions', icon: Briefcase,       label: 'Go to Jobs',       key: 'G J', go: () => navigate('/jobs') },
      { id: 'go-execs',     section: 'Actions', icon: List,            label: 'Go to Executions', key: 'G E', go: () => navigate('/executions') },
      { id: 'go-runners',   section: 'Actions', icon: Cpu,             label: 'Go to Runners',    key: 'G R', go: () => navigate('/runners') },
      { id: 'go-dl',        section: 'Actions', icon: MailX,           label: 'Go to Dead Letters', key: 'G D L', go: () => navigate('/dead-letters') },
      { id: 'go-cals',      section: 'Actions', icon: CalendarDays,    label: 'Go to Calendars',  key: 'G C', go: () => navigate('/calendars') },
      { id: 'go-settings',  section: 'Actions', icon: Settings,        label: 'Go to Settings',   key: 'G S', go: () => navigate('/settings') },
    ]
    const jobMatches: PaletteItem[] = (jobs ?? [])
      .filter((j) => !ql || j.job_key.toLowerCase().includes(ql) || (j.description ?? '').toLowerCase().includes(ql))
      .slice(0, 6)
      .map((j) => ({
        id: 'job-' + j.job_key,
        section: 'Jobs',
        icon: Briefcase,
        label: j.job_key,
        sub: j.description ?? '',
        go: () => navigate(`/jobs/${encodeURIComponent(j.job_key)}`),
      }))
    const runnerMatches: PaletteItem[] = (runners ?? [])
      .filter((r) => !ql || r.runner_id.toLowerCase().includes(ql))
      .slice(0, 4)
      .map((r) => ({
        id: 'rnr-' + r.runner_id,
        section: 'Runners',
        icon: Cpu,
        label: r.runner_id,
        sub: `${r.status}${r.tags.length ? ' • ' + r.tags.join(' ') : ''}`,
        go: () => navigate('/runners'),
      }))
    const all = [...actions, ...jobMatches, ...runnerMatches]
    return ql ? all.filter((it) => (it.label + ' ' + (it.sub ?? '')).toLowerCase().includes(ql)) : all
  }, [q, jobs, runners, navigate])

  function fire(it: PaletteItem | undefined) {
    if (!it) return
    onClose()
    it.go()
  }

  function onKey(e: React.KeyboardEvent<HTMLDivElement>) {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setActive((a) => Math.min(a + 1, items.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActive((a) => Math.max(a - 1, 0))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      fire(items[active])
    } else if (e.key === 'Escape') {
      onClose()
    }
  }

  let lastSection: string | null = null

  return (
    <div
      className="cmd-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="cmd" onKeyDown={onKey} role="dialog" aria-label="Command palette">
        <input
          ref={inputRef}
          className="cmd-input"
          placeholder="Search jobs, runners, or run a command…"
          value={q}
          onChange={(e) => {
            setQ(e.target.value)
            setActive(0)
          }}
        />
        <div className="cmd-list">
          {items.length === 0 ? (
            <div style={{ padding: 20, color: 'var(--fg-3)', fontSize: 13, textAlign: 'center' }}>
              No matches.
            </div>
          ) : null}
          {items.map((it, i) => {
            const showHeader = it.section !== lastSection
            lastSection = it.section
            const Icon = it.icon
            return (
              <div key={it.id}>
                {showHeader ? <div className="cmd-section">{it.section}</div> : null}
                <button
                  type="button"
                  className={clsx('cmd-item', active === i && 'active')}
                  onMouseEnter={() => setActive(i)}
                  onClick={() => fire(it)}
                >
                  <Icon className="icon" />
                  <div className="col" style={{ minWidth: 0, gap: 0 }}>
                    <span style={{ color: 'var(--fg)' }}>{it.label}</span>
                    {it.sub ? <span className="sub">{it.sub}</span> : null}
                  </div>
                  {it.key ? <span className="key">{it.key}</span> : null}
                </button>
              </div>
            )
          })}
        </div>
        <div className="cmd-foot">
          <span>
            <span className="kbd">↵</span> open
          </span>
          <span>
            <span className="kbd">↑↓</span> navigate
          </span>
          <span>
            <span className="kbd">esc</span> close
          </span>
          <span style={{ marginLeft: 'auto' }}>{items.length} results</span>
        </div>
      </div>
    </div>
  )
}
