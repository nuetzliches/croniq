import { useMemo, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { Play, Pencil, Search, Trash2, ExternalLink } from 'lucide-react'
import clsx from 'clsx'
import {
  useJobs,
  useJobTags,
  useJob,
  useJobStats,
  useExecutions,
  useSchedules,
  useTriggerJob,
  useActivateJob,
  useDeactivateJob,
  useDeleteJob,
  useAuditEvents,
} from '@/api/hooks'
import {
  EmptyState,
  StatusPill,
  RunBars,
  Toggle,
  KPICard,
} from '@/components/primitives'
import type { RunOutcome } from '@/components/primitives'
import type { Execution, JobDefinition, TriggerDefinition } from '@/api/types'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { EditJobDialog } from '@/components/EditJobDialog'
import { formatRelative } from '@/lib/utils'

type Tab = 'overview' | 'runs' | 'audit'

const outcomeFor = (e: Execution): RunOutcome => {
  if (e.state === 'completed') return 'ok'
  if (e.state === 'failed' || e.state === 'dead') return 'err'
  if (e.state === 'timeout') return 'warn'
  return 'skip'
}

export function JobsPage() {
  const { jobKey } = useParams<{ jobKey: string }>()
  const navigate = useNavigate()
  const jobs = useJobs()
  const tagCounts = useJobTags()
  const allExecs = useExecutions({ limit: 200 })
  const [search, setSearch] = useState('')
  const [activeTags, setActiveTags] = useState<Set<string>>(new Set())
  const [editing, setEditing] = useState<JobDefinition | null>(null)
  const { confirm, dialog: confirmDialog } = useConfirm()

  const toggleTag = (t: string) =>
    setActiveTags((prev) => {
      const next = new Set(prev)
      if (next.has(t)) next.delete(t)
      else next.add(t)
      return next
    })

  const execsByJob = useMemo(() => {
    const m: Record<string, Execution[]> = {}
    for (const e of allExecs.data ?? []) (m[e.job_key] ??= []).push(e)
    return m
  }, [allExecs.data])

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    return (jobs.data ?? []).filter((j) => {
      if (q && !j.job_key.toLowerCase().includes(q) && !(j.description ?? '').toLowerCase().includes(q)) return false
      if (activeTags.size > 0) {
        const have = new Set(j.tags ?? [])
        for (const t of activeTags) if (!have.has(t)) return false
      }
      return true
    })
  }, [jobs.data, search, activeTags])

  const selected = jobKey ?? (filtered[0]?.job_key ?? null)

  return (
    <div className="split">
      {confirmDialog}
      <EditJobDialog
        job={editing}
        open={editing !== null}
        onOpenChange={(o) => {
          if (!o) setEditing(null)
        }}
      />

      {/* Master list */}
      <aside className="master" aria-label="Jobs list">
        <div
          style={{
            position: 'sticky',
            top: 0,
            background: 'var(--bg-2)',
            padding: '12px 14px 8px',
            zIndex: 2,
            borderBottom: '1px solid var(--divider)',
          }}
        >
          <div className="row between" style={{ marginBottom: 10 }}>
            <h2 className="page-title" style={{ fontSize: 14, fontWeight: 600 }}>
              Jobs
              <span className="dim" style={{ fontSize: 12, marginLeft: 6 }}>
                {filtered.length}/{jobs.data?.length ?? 0}
              </span>
            </h2>
          </div>
          <div style={{ position: 'relative' }}>
            <Search
              size={13}
              style={{
                position: 'absolute',
                left: 10,
                top: '50%',
                transform: 'translateY(-50%)',
                color: 'var(--fg-mute)',
              }}
            />
            <input
              type="search"
              className="input"
              placeholder="Search jobs…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              style={{ paddingLeft: 28 }}
            />
          </div>
          {(tagCounts.data ?? []).length > 0 ? (
            <div className="row" style={{ flexWrap: 'wrap', marginTop: 10, gap: 4 }}>
              {(tagCounts.data ?? []).slice(0, 8).map((tc) => (
                <button
                  key={tc.tag}
                  type="button"
                  className={clsx('tag', activeTags.has(tc.tag) && 'active')}
                  onClick={() => toggleTag(tc.tag)}
                  style={{
                    cursor: 'pointer',
                    background: activeTags.has(tc.tag) ? 'var(--accent-bg)' : undefined,
                    color: activeTags.has(tc.tag) ? 'var(--accent-3)' : undefined,
                    borderColor: activeTags.has(tc.tag) ? 'transparent' : undefined,
                  }}
                >
                  {tc.tag}
                  <span className="dim" style={{ fontSize: 10.5, marginLeft: 4 }}>
                    {tc.count}
                  </span>
                </button>
              ))}
            </div>
          ) : null}
        </div>
        {jobs.isLoading ? (
          <div className="dim center" style={{ padding: 30 }}>
            Loading…
          </div>
        ) : filtered.length === 0 ? (
          <EmptyState
            title="No jobs match"
            desc={search || activeTags.size > 0 ? 'Adjust the search or clear tag filters.' : 'Register a job to get started.'}
          />
        ) : (
          filtered.map((j) => {
            const runs = (execsByJob[j.job_key] ?? []).slice(0, 12)
            const last = runs[0]
            return (
              <button
                key={j.job_key}
                type="button"
                className={clsx('master-row', j.job_key === selected && 'active')}
                onClick={() => navigate(`/jobs/${encodeURIComponent(j.job_key)}`)}
              >
                <div className="row between">
                  <span className="key ellipsis">{j.job_key}</span>
                  <StatusPill state={j.is_active ? 'active' : 'disabled'} dot={false} />
                </div>
                <div className="meta">
                  <RunBars counts={runs.map(outcomeFor).reverse()} compact />
                  <span className="grow" />
                  <span>{last ? formatRelative(last.created_at) : 'no runs'}</span>
                </div>
              </button>
            )
          })
        )}
      </aside>

      {/* Detail */}
      <section className="detail" aria-label="Job detail">
        {selected ? (
          <JobDetail jobKey={selected} onEdit={(j) => setEditing(j)} onDelete={async (k) => {
            const ok = await confirm({
              title: `Delete job ${k}?`,
              description: 'The job, its schedules and trigger state are removed. Past executions and dead letters are preserved.',
              confirmLabel: 'Delete job',
              destructive: true,
            })
            if (ok) {
              await jobs.refetch()
              navigate('/jobs')
            }
          }} />
        ) : (
          <EmptyState
            icon={Search}
            title="Select a job"
            desc="Pick a job from the list on the left to see its overview, runs and audit trail."
          />
        )}
      </section>
    </div>
  )
}

interface JobDetailProps {
  jobKey: string
  onEdit: (j: JobDefinition) => void
  onDelete: (k: string) => Promise<void>
}

function JobDetail({ jobKey, onEdit, onDelete }: JobDetailProps) {
  const [tab, setTab] = useState<Tab>('overview')
  const job = useJob(jobKey)
  const stats = useJobStats(jobKey, 7)
  const schedules = useSchedules(jobKey)
  const executions = useExecutions({ job_key: jobKey, limit: 25 })
  const audit = useAuditEvents({ target_type: 'job', limit: 25 })
  const triggerJob = useTriggerJob()
  const activateJob = useActivateJob()
  const deactivateJob = useDeactivateJob()
  const deleteJob = useDeleteJob()

  if (job.isLoading || !job.data) {
    return (
      <div className="dim center" style={{ padding: 40 }}>
        Loading…
      </div>
    )
  }

  const j = job.data
  const jobAuditEvents = (audit.data ?? []).filter((e) => e.target_id === jobKey)

  const dslManaged = (schedules.data ?? []).some((s) => s.managed_by === 'dsl')

  function setActive(next: boolean) {
    if (next) activateJob.mutate(jobKey)
    else deactivateJob.mutate(jobKey)
  }

  function trigger() {
    triggerJob.mutate(jobKey)
  }

  async function remove() {
    deleteJob.mutate(jobKey)
    await onDelete(jobKey)
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div
        style={{
          padding: '16px 20px',
          borderBottom: '1px solid var(--divider)',
          display: 'flex',
          alignItems: 'flex-start',
          gap: 12,
          flexWrap: 'wrap',
        }}
      >
        <div className="col" style={{ gap: 6, flex: 1, minWidth: 0 }}>
          <div className="row" style={{ gap: 8 }}>
            <h2 className="mono ellipsis" style={{ fontSize: 18, margin: 0, color: 'var(--fg)' }}>
              {j.job_key}
            </h2>
            <StatusPill state={j.is_active ? 'active' : 'disabled'} />
            {dslManaged ? (
              <span className="pill outline" style={{ fontFamily: 'var(--font-mono-app)' }}>
                DSL
              </span>
            ) : null}
          </div>
          {j.description ? (
            <p className="dim" style={{ margin: 0, fontSize: 12.5 }}>
              {j.description}
            </p>
          ) : null}
          {(j.tags ?? []).length > 0 ? (
            <div className="row" style={{ gap: 4, flexWrap: 'wrap', marginTop: 2 }}>
              {(j.tags ?? []).map((t) => (
                <span key={t} className="tag">
                  {t}
                </span>
              ))}
            </div>
          ) : null}
        </div>
        <div className="row" style={{ gap: 6 }}>
          <Toggle on={j.is_active} onChange={setActive} disabled={dslManaged} label="Active" />
          <button type="button" className="btn sm" onClick={trigger} disabled={triggerJob.isPending}>
            <Play size={12} /> Trigger
          </button>
          <button type="button" className="btn sm ghost" onClick={() => onEdit(j)}>
            <Pencil size={12} /> Edit
          </button>
          <Link to={`/jobs/${encodeURIComponent(jobKey)}/edit`} className="btn sm ghost">
            <ExternalLink size={12} /> Advanced
          </Link>
          <button
            type="button"
            className="btn icon sm danger-hover"
            aria-label="Delete"
            title="Delete"
            onClick={remove}
            disabled={dslManaged}
          >
            <Trash2 size={12} />
          </button>
        </div>
      </div>

      {/* Tabs */}
      <div className="tabs">
        <button type="button" className={clsx('tab', tab === 'overview' && 'active')} onClick={() => setTab('overview')}>
          Overview
        </button>
        <button type="button" className={clsx('tab', tab === 'runs' && 'active')} onClick={() => setTab('runs')}>
          Runs <span className="count">{stats.data?.total ?? '—'}</span>
        </button>
        <button type="button" className={clsx('tab', tab === 'audit' && 'active')} onClick={() => setTab('audit')}>
          Audit <span className="count">{jobAuditEvents.length || '—'}</span>
        </button>
      </div>

      {/* Tab content */}
      <div style={{ overflowY: 'auto', flex: 1, padding: 18 }}>
        {tab === 'overview' ? (
          <OverviewTab stats={stats.data} schedules={schedules.data ?? []} executions={executions.data ?? []} />
        ) : null}
        {tab === 'runs' ? <RunsTab executions={executions.data ?? []} loading={executions.isLoading} /> : null}
        {tab === 'audit' ? <AuditTab events={jobAuditEvents} loading={audit.isLoading} /> : null}
      </div>
    </div>
  )
}

function OverviewTab({
  stats,
  schedules,
  executions,
}: {
  stats: ReturnType<typeof useJobStats>['data']
  schedules: TriggerDefinition[]
  executions: Execution[]
}) {
  const successRate =
    stats && stats.total > 0 ? `${(stats.success_rate * 100).toFixed(1)}%` : '—'
  const p50 = stats?.p50_ms ?? null
  const p95 = stats?.p95_ms ?? null

  return (
    <div className="col" style={{ gap: 14 }}>
      <div className="grid cols-4">
        <KPICard title="Success rate (7d)" value={successRate} sub={stats ? <span>{stats.completed} / {stats.total}</span> : null} />
        <KPICard title="Failed (7d)" value={stats?.failed ?? '—'} sub={stats?.dead ? <span className="dim">{stats.dead} dead</span> : <span className="muted">healthy</span>} />
        <KPICard title="p50" value={p50 == null ? '—' : `${p50} ms`} mono />
        <KPICard title="p95" value={p95 == null ? '—' : `${p95} ms`} mono />
      </div>

      <section className="card">
        <div className="card-head">
          <p className="card-title">Schedules</p>
          <span className="dim" style={{ fontSize: 11.5 }}>
            {schedules.length}
          </span>
        </div>
        {schedules.length === 0 ? (
          <p className="dim" style={{ fontSize: 12.5, margin: 0 }}>
            No schedules registered. Use “Advanced” to add one.
          </p>
        ) : (
          <table className="tbl">
            <thead>
              <tr>
                <th>Cron / DSL</th>
                <th>Timezone</th>
                <th>Calendar</th>
                <th>Managed by</th>
                <th>Enabled</th>
              </tr>
            </thead>
            <tbody>
              {schedules.map((s) => (
                <tr key={s.trigger_id}>
                  <td className="mono">{s.cron_expression ?? '—'}</td>
                  <td>{s.timezone ?? '—'}</td>
                  <td>{s.calendar ?? '—'}</td>
                  <td>
                    <span className={clsx('pill', s.managed_by === 'dsl' ? 'outline' : 'accent')}>
                      {s.managed_by}
                    </span>
                  </td>
                  <td>
                    <StatusPill state={s.enabled ? 'enabled' : 'disabled'} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <section className="card">
        <div className="card-head">
          <p className="card-title">Recent runs</p>
          <span className="dim" style={{ fontSize: 11.5 }}>
            last 5
          </span>
        </div>
        <RunsTab executions={executions.slice(0, 5)} loading={false} compact />
      </section>
    </div>
  )
}

function RunsTab({
  executions,
  loading,
  compact = false,
}: {
  executions: Execution[]
  loading: boolean
  compact?: boolean
}) {
  if (loading) {
    return <div className="dim center" style={{ padding: 30 }}>Loading…</div>
  }
  if (executions.length === 0) {
    return <EmptyState title="No runs yet" desc="Trigger the job to see executions here." />
  }
  return (
    <table className="tbl">
      <thead>
        <tr>
          <th>State</th>
          <th>Runner</th>
          <th>Started</th>
          <th>Duration</th>
          {compact ? null : <th>Error</th>}
        </tr>
      </thead>
      <tbody>
        {executions.map((e) => (
          <tr key={e.id}>
            <td>
              <StatusPill state={e.state} />
            </td>
            <td className="mono dim" style={{ fontSize: 11.5 }}>
              {e.runner_id ? e.runner_id.slice(-8) : '—'}
            </td>
            <td>{formatRelative(e.fire_at)}</td>
            <td className="num">{e.duration_ms ? `${e.duration_ms} ms` : '—'}</td>
            {compact ? null : <td className="ellipsis" style={{ maxWidth: 280, color: 'var(--error)' }}>{e.error ?? ''}</td>}
          </tr>
        ))}
      </tbody>
    </table>
  )
}

function AuditTab({
  events,
  loading,
}: {
  events: ReturnType<typeof useAuditEvents>['data']
  loading: boolean
}) {
  if (loading) {
    return <div className="dim center" style={{ padding: 30 }}>Loading…</div>
  }
  if (!events || events.length === 0) {
    return <EmptyState title="No audit events" desc="Mutations to this job will be logged here." />
  }
  return (
    <table className="tbl">
      <thead>
        <tr>
          <th>When</th>
          <th>Action</th>
          <th>Actor</th>
        </tr>
      </thead>
      <tbody>
        {events.map((e) => (
          <tr key={e.event_id}>
            <td className="dim mono" style={{ fontSize: 11.5 }}>
              {formatRelative(e.created_at)}
            </td>
            <td className="mono">{e.action}</td>
            <td className="dim">
              {e.actor_type}
              {e.actor_id ? <span className="dim mono"> · {e.actor_id.slice(0, 8)}</span> : null}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}
