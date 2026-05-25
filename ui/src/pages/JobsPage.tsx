import { useEffect, useMemo, useState } from 'react'
import { useNavigate, useParams } from 'react-router'
import {
  Play,
  Pencil,
  Search,
  Trash2,
  Plus,
  RotateCcw,
  CalendarDays,
  Bell,
} from 'lucide-react'
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
  useForecast,
} from '@/api/hooks'
import {
  EmptyState,
  StatusPill,
  RunBars,
  Toggle,
  KPICard,
  Sparkline,
  CopyBtn,
  Avatar,
  BrandMark,
} from '@/components/primitives'
import type { RunOutcome } from '@/components/primitives'
import type { AuditEvent, Execution, JobDefinition, TriggerDefinition } from '@/api/types'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { EditJobDialog } from '@/components/EditJobDialog'
import { formatRelative, formatDate } from '@/lib/utils'
import { useCurrentUser } from '@/api/hooks'

type Tab = 'overview' | 'runs' | 'schedule' | 'dsl' | 'alerts' | 'audit'

const TABS: { id: Tab; label: string }[] = [
  { id: 'overview', label: 'Overview' },
  { id: 'runs', label: 'Runs' },
  { id: 'schedule', label: 'Schedule' },
  { id: 'dsl', label: 'DSL' },
  { id: 'alerts', label: 'Alerts' },
  { id: 'audit', label: 'Audit' },
]

const outcomeFor = (e: Execution): RunOutcome => {
  if (e.state === 'completed') return 'ok'
  if (e.state === 'failed' || e.state === 'dead') return 'err'
  if (e.state === 'timeout') return 'warn'
  return 'skip'
}

function durationFmt(ms: number | null): string {
  if (ms == null) return '—'
  if (ms < 1000) return `${ms} ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`
  return `${Math.floor(ms / 60_000)}m ${Math.floor((ms % 60_000) / 1000)}s`
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

  async function handleDelete(k: string) {
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
  }

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

      <aside className="master" aria-label="Jobs list">
        <div
          className="master-filter"
          style={{
            padding: '12px 14px 10px',
            display: 'flex',
            flexDirection: 'column',
            gap: 10,
          }}
        >
          <div className="row gap-6">
            <div style={{ position: 'relative', flex: 1 }}>
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
                placeholder="Filter jobs…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                style={{ paddingLeft: 28 }}
              />
            </div>
            <button
              type="button"
              className="btn icon"
              title="New job"
              onClick={() => navigate('/jobs')}
              aria-label="New job"
            >
              <Plus size={14} />
            </button>
          </div>
          {(tagCounts.data ?? []).length > 0 ? (
            <div className="row gap-6" style={{ flexWrap: 'wrap' }}>
              <button
                type="button"
                className={clsx('pill', activeTags.size === 0 ? 'accent' : 'outline')}
                onClick={() => setActiveTags(new Set())}
              >
                <span className="dot" /> All <span>{jobs.data?.length ?? 0}</span>
              </button>
              {(tagCounts.data ?? []).slice(0, 8).map((tc) => (
                <button
                  key={tc.tag}
                  type="button"
                  className={clsx('pill', activeTags.has(tc.tag) ? 'accent' : 'outline')}
                  onClick={() => toggleTag(tc.tag)}
                >
                  {tc.tag} <span className="dim">{tc.count}</span>
                </button>
              ))}
            </div>
          ) : null}
        </div>

        <div className="master-list">
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
            filtered.map((j) => (
              <JobRow
                key={j.job_key}
                job={j}
                active={j.job_key === selected}
                execs={execsByJob[j.job_key] ?? []}
                onClick={() => navigate(`/jobs/${encodeURIComponent(j.job_key)}`)}
              />
            ))
          )}
        </div>
      </aside>

      <section className="detail" aria-label="Job detail">
        {selected ? (
          <JobDetailContent
            jobKey={selected}
            onEdit={(j) => setEditing(j)}
            onDelete={handleDelete}
          />
        ) : (
          <EmptyState
            icon={Search}
            title="Select a job"
            desc="Pick a job from the list on the left to see its overview, runs, schedule, DSL and audit trail."
          />
        )}
      </section>
    </div>
  )
}

function JobRow({
  job,
  active,
  execs,
  onClick,
}: {
  job: JobDefinition
  active: boolean
  execs: Execution[]
  onClick: () => void
}) {
  const recent = execs.slice(0, 14)
  const total = recent.length
  const fails = recent.filter((e) => e.state === 'failed' || e.state === 'dead').length
  const failRate = total === 0 ? 0 : fails / total

  return (
    <button type="button" className={clsx('job-row', active && 'active')} onClick={onClick}>
      <div className="row between">
        <span className="key ellipsis" style={{ minWidth: 0, flex: 1 }}>
          {job.job_key}
        </span>
        {!job.is_active ? <StatusPill state="disabled" dot={false} /> : null}
      </div>
      <div className="dim ellipsis" style={{ fontSize: 11.5 }}>
        {job.description || '—'}
      </div>
      <div className="row between">
        <div className="meta ellipsis" style={{ minWidth: 0, flex: 1 }}>
          {(job.tags ?? []).slice(0, 2).map((t) => (
            <span key={t} className="dim mono" style={{ marginRight: 6 }}>
              #{t}
            </span>
          ))}
        </div>
        <div className="row gap-6" style={{ flexShrink: 0 }}>
          <RunBars
            counts={recent.map(outcomeFor).reverse()}
            durations={recent.map((e) => e.duration_ms).reverse()}
            compact
          />
          <span
            className="mono"
            style={{
              fontSize: 10.5,
              color: failRate > 0 ? 'var(--error)' : 'var(--success)',
              minWidth: 32,
              textAlign: 'right',
            }}
          >
            {total === 0 ? '—' : failRate > 0 ? `${(failRate * 100).toFixed(0)}%` : '100%'}
          </span>
        </div>
      </div>
    </button>
  )
}

interface JobDetailProps {
  jobKey: string
  onEdit: (j: JobDefinition) => void
  onDelete: (k: string) => Promise<void>
}

function JobDetailContent({ jobKey, onEdit, onDelete }: JobDetailProps) {
  const [tab, setTab] = useState<Tab>('overview')
  const job = useJob(jobKey)
  const stats = useJobStats(jobKey, 7)
  const schedules = useSchedules(jobKey)
  const executions = useExecutions({ job_key: jobKey, limit: 30 })
  const audit = useAuditEvents({ target_type: 'job', limit: 50 })
  const forecast = useForecast(180)
  const triggerJob = useTriggerJob()
  const activateJob = useActivateJob()
  const deactivateJob = useDeactivateJob()
  const deleteJob = useDeleteJob()

  const execsData = executions.data
  // p95 sparkline: derive a duration series from the most recent 24
  // completed executions in fire-order. If we don't have any duration
  // data, the sparkline gracefully renders empty.
  const durSeries = useMemo(
    () =>
      (execsData ?? [])
        .filter((e) => e.duration_ms != null)
        .slice(0, 24)
        .map((e) => e.duration_ms ?? 0)
        .reverse(),
    [execsData],
  )

  // Find the next scheduled fire for this job from the global forecast.
  const forecastData = forecast.data
  const nextFire = useMemo(() => {
    const next = (forecastData?.buckets ?? [])
      .map((b) => b as unknown as { job_key?: string; start: string })
      .find((b) => b.job_key === jobKey)
    return next?.start ?? null
  }, [forecastData, jobKey])

  if (job.isLoading || !job.data) {
    return <div className="dim center" style={{ padding: 40 }}>Loading…</div>
  }
  const j = job.data
  const dslManaged = (schedules.data ?? []).some((s) => s.managed_by === 'dsl')
  const execs = execsData ?? []
  const last20 = execs.slice(0, 20)
  const jobAudit = (audit.data ?? []).filter((e) => e.target_id === jobKey)

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
    <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--card-gap)' }}>
      <div className="card" style={{ padding: 0, overflow: 'visible' }}>
        <JobDetailHeader
          job={j}
          dslManaged={dslManaged}
          triggerPending={triggerJob.isPending}
          onToggle={setActive}
          onTrigger={trigger}
          onEdit={() => onEdit(j)}
          onRemove={remove}
        />
      </div>

      <KpiRow
        runsLast24={last20}
        stats={stats.data ?? null}
        durSeries={durSeries}
        nextFire={nextFire}
        schedule={schedules.data?.[0] ?? null}
      />

      <div className="card" style={{ padding: 0 }}>
        <div className="tabs" style={{ padding: '0 20px', borderBottom: '1px solid var(--border)' }}>
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              className={clsx('tab', tab === t.id && 'active')}
              onClick={() => setTab(t.id)}
            >
              {t.label}
              {t.id === 'runs' && stats.data ? <span className="count">{stats.data.total}</span> : null}
              {t.id === 'audit' && jobAudit.length > 0 ? <span className="count">{jobAudit.length}</span> : null}
            </button>
          ))}
        </div>
        <div style={{ padding: '18px 20px 24px' }}>
          {tab === 'overview' ? (
            <OverviewTab
              job={j}
              executions={execs}
              schedules={schedules.data ?? []}
            />
          ) : null}
          {tab === 'runs' ? <RunsTab executions={execs} loading={executions.isLoading} /> : null}
          {tab === 'schedule' ? <ScheduleTab schedules={schedules.data ?? []} loading={schedules.isLoading} /> : null}
          {tab === 'dsl' ? <DslTab job={j} schedules={schedules.data ?? []} /> : null}
          {tab === 'alerts' ? <AlertsTab /> : null}
          {tab === 'audit' ? <AuditTab events={jobAudit} loading={audit.isLoading} /> : null}
        </div>
      </div>
    </div>
  )
}

function JobDetailHeader({
  job,
  dslManaged,
  triggerPending,
  onToggle,
  onTrigger,
  onEdit,
  onRemove,
}: {
  job: JobDefinition
  dslManaged: boolean
  triggerPending: boolean
  onToggle: (next: boolean) => void
  onTrigger: () => void
  onEdit: () => void
  onRemove: () => void
}) {
  return (
    <div
      style={{
        display: 'flex',
        flexWrap: 'wrap',
        gap: 14,
        alignItems: 'flex-start',
        justifyContent: 'space-between',
        padding: '16px 20px',
      }}
    >
      <div className="col" style={{ gap: 6, minWidth: 0, flex: '1 1 380px' }}>
        <div className="row gap-8" style={{ flexWrap: 'wrap' }}>
          <h1
            className="mono"
            style={{
              margin: 0,
              fontSize: 22,
              fontWeight: 600,
              letterSpacing: '-0.01em',
              wordBreak: 'break-word',
              color: 'var(--fg)',
            }}
          >
            {job.job_key}
          </h1>
          <StatusPill state={job.is_active ? 'active' : 'disabled'} />
          {dslManaged ? (
            <span className="pill outline" style={{ fontFamily: 'var(--font-mono-app)' }}>
              DSL
            </span>
          ) : null}
          <CopyBtn value={job.job_key} />
        </div>
        <div className="row gap-8" style={{ flexWrap: 'wrap' }}>
          {(job.tags ?? []).map((t) => (
            <span key={t} className="tag">
              {t}
            </span>
          ))}
          {dslManaged ? (
            <>
              <span className="dim" style={{ fontSize: 12 }}>·</span>
              <span className="dim" style={{ fontSize: 12 }}>
                managed by Croniqfile
              </span>
            </>
          ) : null}
        </div>
        {job.description ? (
          <p
            style={{
              margin: 0,
              color: 'var(--fg-1)',
              fontSize: 13.5,
              maxWidth: 720,
            }}
          >
            {job.description}
          </p>
        ) : null}
      </div>
      <div className="row gap-6" style={{ flexShrink: 0, flexWrap: 'wrap', justifyContent: 'flex-end' }}>
        <Toggle on={job.is_active} onChange={onToggle} disabled={dslManaged} label="Active" />
        <button type="button" className="btn sm ghost" onClick={onEdit}>
          <Pencil size={13} /> Edit
        </button>
        <button type="button" className="btn sm primary" onClick={onTrigger} disabled={triggerPending}>
          {triggerPending ? <BrandMark spinning size={13} /> : <Play size={13} />} Trigger
        </button>
        <button
          type="button"
          className="btn icon sm danger-hover"
          aria-label="Delete"
          title={dslManaged ? 'DSL-managed jobs cannot be deleted via the UI' : 'Delete'}
          onClick={onRemove}
          disabled={dslManaged}
        >
          <Trash2 size={13} />
        </button>
      </div>
    </div>
  )
}

function KpiRow({
  runsLast24,
  stats,
  durSeries,
  nextFire,
  schedule,
}: {
  runsLast24: Execution[]
  stats: ReturnType<typeof useJobStats>['data'] | null
  durSeries: number[]
  nextFire: string | null
  schedule: TriggerDefinition | null
}) {
  const sr = stats && stats.total > 0 ? stats.success_rate * 100 : null
  const srColor =
    sr === null ? 'var(--fg)' : sr === 100 ? 'var(--success)' : sr > 90 ? 'var(--fg)' : 'var(--error)'

  // Tick once per minute so the "in 12m 14s" countdown stays fresh
  // without making the useMemo body impure (Date.now lives in the
  // effect, not the render path).
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 60_000)
    return () => window.clearInterval(t)
  }, [])

  const fireRel = useMemo(() => {
    if (!nextFire) return null
    const ms = +new Date(nextFire) - now
    if (ms <= 0) return 'now'
    const secs = Math.floor(ms / 1000)
    if (secs < 60) return `in ${secs}s`
    if (secs < 3600) return `in ${Math.floor(secs / 60)}m ${secs % 60}s`
    return `in ${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`
  }, [nextFire, now])

  return (
    <div className="grid cols-4">
      <KPICard
        title="Last 24h"
        value={runsLast24.length}
        mono
        sub={
          <RunBars
            counts={runsLast24.map(outcomeFor).reverse()}
            durations={runsLast24.map((e) => e.duration_ms).reverse()}
            compact
          />
        }
      />
      <KPICard
        title="Success rate"
        value={
          <span style={{ color: srColor }}>
            {sr === null ? '—' : `${sr.toFixed(1)}%`}
          </span>
        }
        mono
        sub={
          stats ? (
            <span>
              {stats.completed} ok · {stats.failed} fail
              {stats.dead > 0 ? <span className="dim"> · {stats.dead} dead</span> : null}
            </span>
          ) : (
            <span className="muted">—</span>
          )
        }
      />
      <KPICard
        title="p95 duration"
        value={stats?.p95_ms != null ? durationFmt(stats.p95_ms) : '—'}
        mono
        chart={durSeries.length > 1 ? <Sparkline data={durSeries} color="var(--accent)" height={32} /> : null}
      />
      <KPICard
        title="Next fire"
        value={<span style={{ fontSize: 18 }}>{fireRel ?? '—'}</span>}
        mono
        sub={
          schedule ? (
            <span className="mono dim" style={{ fontSize: 11 }}>
              {schedule.cron_expression ?? '—'}
              {schedule.timezone ? ` · ${schedule.timezone}` : ''}
            </span>
          ) : (
            <span className="muted">no schedule</span>
          )
        }
      />
    </div>
  )
}

function OverviewTab({
  job,
  executions,
  schedules,
}: {
  job: JobDefinition
  executions: Execution[]
  schedules: TriggerDefinition[]
}) {
  const { data: me } = useCurrentUser()
  const ownerName = me?.display_name || me?.username || 'system'
  const ownerEmail = me?.email ?? ''
  const firstSched = schedules[0] ?? null

  return (
    <div className="job-overview-grid">
      <section className="card" style={{ padding: 0 }}>
        <div className="row between" style={{ padding: '14px 16px 8px' }}>
          <p className="card-title">Recent runs</p>
          <span className="dim" style={{ fontSize: 11.5 }}>
            last {Math.min(executions.length, 12)}
          </span>
        </div>
        {executions.length === 0 ? (
          <EmptyState title="No runs yet" desc="Trigger the job to see executions here." />
        ) : (
          <table className="tbl">
            <thead>
              <tr>
                <th>ID</th>
                <th>State</th>
                <th>Runner</th>
                <th>Fire at</th>
                <th>Duration</th>
              </tr>
            </thead>
            <tbody>
              {executions.slice(0, 12).map((e) => (
                <tr key={e.id}>
                  <td className="mono dim" style={{ fontSize: 11.5 }} title={e.id}>
                    {e.id.slice(0, 8)}
                  </td>
                  <td>
                    <StatusPill state={e.state} />
                  </td>
                  <td className="mono dim" style={{ fontSize: 11.5 }}>
                    {e.runner_id ? e.runner_id.slice(-8) : '—'}
                  </td>
                  <td className="dim">{formatRelative(e.fire_at)}</td>
                  <td className="num">{durationFmt(e.duration_ms)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <div className="col" style={{ gap: 14 }}>
        <section className="card">
          <div className="card-title" style={{ marginBottom: 12 }}>
            Schedule
          </div>
          <div className="col" style={{ gap: 8 }}>
            <DetailRow label="Cron / DSL" value={<span className="mono" style={{ color: 'var(--fg)' }}>{firstSched?.cron_expression ?? '—'}</span>} />
            <DetailRow label="Timezone" value={<span className="mono">{firstSched?.timezone ?? '—'}</span>} />
            <DetailRow
              label="Calendar"
              value={
                firstSched?.calendar ? (
                  <span className="mono" style={{ color: 'var(--accent-3)' }}>{firstSched.calendar}</span>
                ) : (
                  <span className="dim">—</span>
                )
              }
            />
            <DetailRow label="Timeout" value={<span className="mono">{job.timeout ?? '—'}</span>} />
            <DetailRow label="Managed by" value={<span className="mono">{firstSched?.managed_by ?? '—'}</span>} />
          </div>
        </section>

        <section className="card">
          <div className="card-title" style={{ marginBottom: 12 }}>
            Routing
          </div>
          <div className="col" style={{ gap: 8 }}>
            <DetailRow
              label="Assigned runner"
              value={
                job.assigned_runner_id ? (
                  <span className="mono">{job.assigned_runner_id.slice(-8)}</span>
                ) : (
                  <span className="dim">any</span>
                )
              }
            />
            <DetailRow label="Max retries" value={<span className="mono">{job.max_retries ?? '—'}</span>} />
            <DetailRow
              label="Dead letter"
              value={
                <StatusPill state={job.dead_letter_enabled ? 'enabled' : 'disabled'} />
              }
            />
          </div>
        </section>

        <section className="card">
          <div className="card-title" style={{ marginBottom: 12 }}>
            Owned by
          </div>
          <div className="row gap-8">
            <Avatar name={ownerName} />
            <div className="col" style={{ gap: 0 }}>
              <div>{ownerName}</div>
              {ownerEmail ? (
                <div className="dim" style={{ fontSize: 11.5 }}>
                  {ownerEmail}
                </div>
              ) : null}
            </div>
          </div>
        </section>
      </div>
    </div>
  )
}

function DetailRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="row between" style={{ fontSize: 13 }}>
      <span className="dim">{label}</span>
      <span>{value}</span>
    </div>
  )
}

function RunsTab({ executions, loading }: { executions: Execution[]; loading: boolean }) {
  if (loading) {
    return <div className="dim center" style={{ padding: 30 }}>Loading…</div>
  }
  if (executions.length === 0) {
    return <EmptyState title="No runs yet" desc="Trigger the job to see executions here." />
  }
  return (
    <section className="card" style={{ padding: 0 }}>
      <table className="tbl">
        <thead>
          <tr>
            <th>ID</th>
            <th>State</th>
            <th>Runner</th>
            <th>Fire at</th>
            <th>Duration</th>
            <th>Attempt</th>
            <th>Error</th>
          </tr>
        </thead>
        <tbody>
          {executions.map((e) => (
            <tr key={e.id}>
              <td className="mono dim" style={{ fontSize: 11.5 }} title={e.id}>
                {e.id.slice(0, 8)}
              </td>
              <td>
                <StatusPill state={e.state} />
              </td>
              <td className="mono dim" style={{ fontSize: 11.5 }}>
                {e.runner_id ? e.runner_id.slice(-8) : '—'}
              </td>
              <td className="dim">{formatRelative(e.fire_at)}</td>
              <td className="num">{durationFmt(e.duration_ms)}</td>
              <td className="num">{e.attempt}</td>
              <td className="ellipsis" style={{ maxWidth: 240, color: 'var(--error)' }}>
                {e.error ?? ''}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  )
}

function ScheduleTab({
  schedules,
  loading,
}: {
  schedules: TriggerDefinition[]
  loading: boolean
}) {
  if (loading) {
    return <div className="dim center" style={{ padding: 30 }}>Loading…</div>
  }
  if (schedules.length === 0) {
    return (
      <EmptyState
        icon={CalendarDays}
        title="No schedules"
        desc="Open the advanced editor to attach a cron expression or DSL rule."
      />
    )
  }
  return (
    <section className="card" style={{ padding: 0 }}>
      <table className="tbl">
        <thead>
          <tr>
            <th>Cron / DSL</th>
            <th>Timezone</th>
            <th>Calendar</th>
            <th>Window</th>
            <th>Managed by</th>
            <th>Enabled</th>
            <th>Updated</th>
          </tr>
        </thead>
        <tbody>
          {schedules.map((s) => (
            <tr key={s.trigger_id}>
              <td className="mono">{s.cron_expression ?? '—'}</td>
              <td>{s.timezone ?? '—'}</td>
              <td>{s.calendar ?? '—'}</td>
              <td>{s.window ?? '—'}</td>
              <td>
                <span className={clsx('pill', s.managed_by === 'dsl' ? 'outline' : 'accent')}>
                  {s.managed_by}
                </span>
              </td>
              <td>
                <StatusPill state={s.enabled ? 'enabled' : 'disabled'} />
              </td>
              <td className="dim">{formatRelative(s.updated_at)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  )
}

function DslTab({
  job,
  schedules,
}: {
  job: JobDefinition
  schedules: TriggerDefinition[]
}) {
  const dsl = useMemo(() => renderDsl(job, schedules), [job, schedules])
  return (
    <section className="card" style={{ padding: 0 }}>
      <div className="between" style={{ padding: '10px 14px', borderBottom: '1px solid var(--border)' }}>
        <div className="mono dim" style={{ fontSize: 12 }}>
          Croniqfile
        </div>
        <div className="row gap-6">
          <CopyBtn value={dsl} label="Copy" />
        </div>
      </div>
      <pre
        style={{
          margin: 0,
          padding: '14px 16px',
          fontFamily: 'var(--font-mono-app)',
          fontSize: 12.5,
          color: 'var(--fg-1)',
          whiteSpace: 'pre-wrap',
          lineHeight: 1.65,
        }}
      >
        {dsl}
      </pre>
    </section>
  )
}

function renderDsl(job: JobDefinition, schedules: TriggerDefinition[]): string {
  const tags = JSON.stringify(job.tags ?? [])
  const timeout = job.timeout ?? '5m'
  const sched = schedules[0]
  return [
    `# ${job.job_key}`,
    `# rendered from the live job + first attached schedule`,
    ``,
    `job "${job.job_key}" {`,
    `  description = ${JSON.stringify(job.description ?? '')}`,
    `  tags        = ${tags}`,
    `  timeout     = "${timeout}"`,
    ...(job.max_retries != null ? [`  max_retries = ${job.max_retries}`] : []),
    ...(sched
      ? [
          ``,
          `  schedule {`,
          `    rule = ${JSON.stringify(sched.cron_expression ?? '')}`,
          `    tz   = "${sched.timezone ?? 'UTC'}"`,
          ...(sched.calendar ? [`    calendar = "${sched.calendar}"`] : []),
          ...(sched.window ? [`    window   = "${sched.window}"`] : []),
          `  }`,
        ]
      : []),
    `}`,
    ``,
  ].join('\n')
}

function AlertsTab() {
  return (
    <EmptyState
      icon={Bell}
      title="Alerts are not wired yet"
      desc="The /v1/alerts endpoints are planned for a follow-up PR. Attach Slack / email channels per job once the backend ships them."
    />
  )
}

function AuditTab({ events, loading }: { events: AuditEvent[]; loading: boolean }) {
  if (loading) {
    return <div className="dim center" style={{ padding: 30 }}>Loading…</div>
  }
  if (events.length === 0) {
    return <EmptyState title="No audit events" desc="Mutations to this job will appear here." />
  }
  return (
    <section className="card">
      <div className="row between" style={{ marginBottom: 10 }}>
        <p className="card-title">Audit log</p>
        <span className="dim" style={{ fontSize: 11.5 }}>
          {events.length} events
        </span>
      </div>
      <div className="audit-timeline">
        {events.map((e) => {
          const kind = auditKind(e.action)
          const Icon = auditIcon(kind)
          return (
            <div key={e.event_id} className="audit-event">
              <div className={`audit-marker audit-${kind}`}>
                <Icon size={13} />
              </div>
              <div className="audit-content">
                <div className="row gap-8" style={{ alignItems: 'baseline', flexWrap: 'wrap' }}>
                  <span style={{ color: 'var(--fg)', fontWeight: 500, fontSize: 13 }}>
                    {e.actor_type}
                    {e.actor_id ? <span className="dim mono" style={{ fontWeight: 400, marginLeft: 4 }}>· {e.actor_id.slice(0, 8)}</span> : null}
                  </span>
                  <span className="dim" style={{ fontSize: 13 }}>
                    {humanizeAction(e.action)}
                  </span>
                  {e.target_id ? (
                    <span className="mono" style={{ color: 'var(--accent-3)', fontSize: 12.5 }}>
                      {e.target_id.slice(0, 8)}
                    </span>
                  ) : null}
                </div>
                <div className="row gap-6 dim mono" style={{ fontSize: 11 }}>
                  <span>{formatDate(e.created_at)}</span>
                  <span>·</span>
                  <span>{formatRelative(e.created_at)}</span>
                </div>
              </div>
            </div>
          )
        })}
      </div>
    </section>
  )
}

type AuditKind = 'sync' | 'edit' | 'trigger' | 'create' | 'delete'

function auditKind(action: string): AuditKind {
  if (action.includes('trigger')) return 'trigger'
  if (action.includes('create') || action.includes('register')) return 'create'
  if (action.includes('delete') || action.includes('revoke')) return 'delete'
  if (action.includes('dsl') || action.includes('sync')) return 'sync'
  return 'edit'
}

function auditIcon(kind: AuditKind): typeof Edit3 {
  if (kind === 'trigger') return Play
  if (kind === 'create') return Plus
  if (kind === 'delete') return Trash2
  if (kind === 'sync') return RotateCcw
  return Edit3
}

function humanizeAction(action: string): string {
  return action.replace(/[._]/g, ' ')
}
