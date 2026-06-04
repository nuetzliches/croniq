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
  Edit3,
  Download,
  Upload,
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
  useAdoptJob,
  useUnadoptJob,
  useDeleteSchedule,
  useAuditEvents,
  useForecast,
  useAlertDeliveries,
} from '@/api/hooks'
import { DeliveriesList } from '@/pages/AlertsPage'
import {
  EmptyState,
  StatusPill,
  ExecutionBars,
  Toggle,
  KPICard,
  Sparkline,
  CopyBtn,
  Avatar,
  BrandMark,
} from '@/components/primitives'
import type { ExecutionOutcome } from '@/components/primitives'
import type { AuditEvent, Execution, JobDefinition, TriggerDefinition } from '@/api/types'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { EditJobDialog } from '@/components/EditJobDialog'
import { NewJobDialog } from '@/components/NewJobDialog'
import { ScheduleDialog } from '@/components/ScheduleDialog'
import { formatRelative, formatDate } from '@/lib/utils'
import { useCurrentUser } from '@/api/hooks'

type Tab = 'overview' | 'executions' | 'schedule' | 'dsl' | 'alerts' | 'audit'

const TABS: { id: Tab; label: string }[] = [
  { id: 'overview', label: 'Overview' },
  { id: 'executions', label: 'Executions' },
  { id: 'schedule', label: 'Schedule' },
  { id: 'dsl', label: 'DSL' },
  { id: 'alerts', label: 'Alerts' },
  { id: 'audit', label: 'Audit' },
]

const outcomeFor = (e: Execution): ExecutionOutcome => {
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
  const [creating, setCreating] = useState(false)
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
      <NewJobDialog open={creating} onOpenChange={setCreating} />

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
              onClick={() => setCreating(true)}
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
            desc="Pick a job from the list on the left to see its overview, executions, schedule, DSL and audit trail."
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
  // Success-rate semantics, matching the KpiRow in the detail header:
  // 100% green, >= 90% neutral, < 90% red. The previous version mixed
  // failure-rate (red) with success-rate (green) on the same chip, which
  // made a job with one failure read as "7%" — confusing it for a
  // success score in the low single digits.
  const successRate = total === 0 ? null : (total - fails) / total
  const srColor =
    successRate === null
      ? 'var(--fg-mute)'
      : successRate === 1
        ? 'var(--success)'
        : successRate >= 0.9
          ? 'var(--fg)'
          : 'var(--error)'

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
          <ExecutionBars
            counts={recent.map(outcomeFor).reverse()}
            durations={recent.map((e) => e.duration_ms).reverse()}
            compact
          />
          <span
            className="mono"
            style={{
              fontSize: 10.5,
              color: srColor,
              minWidth: 32,
              textAlign: 'right',
            }}
            title={total === 0 ? 'No recent executions' : `${total - fails}/${total} successful`}
          >
            {successRate === null ? '—' : `${(successRate * 100).toFixed(0)}%`}
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
  const adoptJob = useAdoptJob()
  const unadoptJob = useUnadoptJob()
  const deleteSchedule = useDeleteSchedule()
  const { confirm, dialog: confirmDialog } = useConfirm()
  const [scheduleEditing, setScheduleEditing] = useState<TriggerDefinition | null>(null)
  const [scheduleCreating, setScheduleCreating] = useState(false)
  const [adoptError, setAdoptError] = useState<string | null>(null)

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

  async function handleAdopt() {
    const ok = await confirm({
      title: `Adopt job ${jobKey}?`,
      description:
        'A copy of this job and its schedule is created in the API store. The Croniqfile definition is ignored on the next reload until you unadopt. Requires `policy { dsl_adopt_on_mutate true }`.',
      confirmLabel: 'Adopt to edit',
    })
    if (!ok) return
    setAdoptError(null)
    try {
      await adoptJob.mutateAsync(jobKey)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      const m = msg.match(/^409:\s*(.+)$/s)
      if (m) {
        try {
          const parsed = JSON.parse(m[1])
          setAdoptError(parsed.message ?? msg)
        } catch {
          setAdoptError(m[1])
        }
      } else {
        setAdoptError(msg)
      }
    }
  }

  async function handleUnadopt() {
    const ok = await confirm({
      title: `Unadopt job ${jobKey}?`,
      description:
        'The API copy is dropped. The next config reload reinstates the Croniqfile definition; any UI-only edits to the job or its schedule are lost.',
      confirmLabel: 'Unadopt',
      destructive: true,
    })
    if (!ok) return
    await unadoptJob.mutateAsync(jobKey)
  }

  async function handleDeleteSchedule(s: TriggerDefinition) {
    const ok = await confirm({
      title: 'Delete schedule?',
      description: `The trigger ${s.cron_expression ?? s.trigger_id} will be removed. Past executions are preserved.`,
      confirmLabel: 'Delete schedule',
      destructive: true,
    })
    if (ok) await deleteSchedule.mutateAsync(s.trigger_id)
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--card-gap)' }}>
      {confirmDialog}
      <ScheduleDialog
        jobKey={jobKey}
        schedule={scheduleEditing}
        open={scheduleEditing !== null || scheduleCreating}
        onOpenChange={(o) => {
          if (!o) {
            setScheduleEditing(null)
            setScheduleCreating(false)
          }
        }}
      />
      <div className="card" style={{ padding: 0, overflow: 'visible' }}>
        <JobDetailHeader
          job={j}
          dslManaged={dslManaged}
          triggerPending={triggerJob.isPending}
          adoptPending={adoptJob.isPending}
          unadoptPending={unadoptJob.isPending}
          onToggle={setActive}
          onTrigger={trigger}
          onEdit={() => onEdit(j)}
          onRemove={remove}
          onAdopt={handleAdopt}
          onUnadopt={handleUnadopt}
        />
        {adoptError ? (
          <div className="row" style={{ padding: '0 20px 12px', color: 'var(--error)', fontSize: 12 }}>
            {adoptError}
          </div>
        ) : null}
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
              {t.id === 'executions' && stats.data ? <span className="count">{stats.data.total}</span> : null}
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
          {tab === 'executions' ? <ExecutionsTab executions={execs} loading={executions.isLoading} /> : null}
          {tab === 'schedule' ? (
            <ScheduleTab
              schedules={schedules.data ?? []}
              loading={schedules.isLoading}
              dslManaged={dslManaged}
              onAdd={() => setScheduleCreating(true)}
              onEdit={(s) => setScheduleEditing(s)}
              onDelete={handleDeleteSchedule}
            />
          ) : null}
          {tab === 'dsl' ? <DslTab job={j} schedules={schedules.data ?? []} /> : null}
          {tab === 'alerts' ? <AlertsTab jobKey={jobKey} /> : null}
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
  adoptPending,
  unadoptPending,
  onToggle,
  onTrigger,
  onEdit,
  onRemove,
  onAdopt,
  onUnadopt,
}: {
  job: JobDefinition
  dslManaged: boolean
  triggerPending: boolean
  adoptPending: boolean
  unadoptPending: boolean
  onToggle: (next: boolean) => void
  onTrigger: () => void
  onEdit: () => void
  onRemove: () => void
  onAdopt: () => void
  onUnadopt: () => void
}) {
  // Adopt is offered for DSL-managed jobs. Unadopt is offered for the
  // API-store copy of a previously-adopted job — detection-by-metadata
  // is not exposed yet, so we surface the button for any non-DSL job
  // and let the backend reject with 404 if there's no adoption record.
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
        {dslManaged ? (
          <button
            type="button"
            className="btn sm ghost"
            onClick={onAdopt}
            disabled={adoptPending}
            title="Copy this DSL job into the API store so it can be edited. Requires policy { dsl_adopt_on_mutate true } in the Croniqfile."
          >
            {adoptPending ? <BrandMark spinning size={13} /> : <Download size={13} />} Adopt
          </button>
        ) : (
          <button
            type="button"
            className="btn sm ghost"
            onClick={onUnadopt}
            disabled={unadoptPending}
            title="Drop the API copy so the next config reload reinstates the Croniqfile definition. No-op if the job was never adopted."
          >
            {unadoptPending ? <BrandMark spinning size={13} /> : <Upload size={13} />} Unadopt
          </button>
        )}
        <button type="button" className="btn sm ghost" onClick={onEdit} disabled={dslManaged}>
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
          <ExecutionBars
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
          <p className="card-title">Recent executions</p>
          <span className="dim" style={{ fontSize: 11.5 }}>
            last {Math.min(executions.length, 12)}
          </span>
        </div>
        {executions.length === 0 ? (
          <EmptyState title="No executions yet" desc="Trigger the job to see executions here." />
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

function ExecutionsTab({ executions, loading }: { executions: Execution[]; loading: boolean }) {
  if (loading) {
    return <div className="dim center" style={{ padding: 30 }}>Loading…</div>
  }
  if (executions.length === 0) {
    return <EmptyState title="No executions yet" desc="Trigger the job to see executions here." />
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
  dslManaged,
  onAdd,
  onEdit,
  onDelete,
}: {
  schedules: TriggerDefinition[]
  loading: boolean
  dslManaged: boolean
  onAdd: () => void
  onEdit: (s: TriggerDefinition) => void
  onDelete: (s: TriggerDefinition) => void
}) {
  if (loading) {
    return <div className="dim center" style={{ padding: 30 }}>Loading…</div>
  }
  return (
    <div className="col" style={{ gap: 12 }}>
      <div className="row between">
        <span className="dim" style={{ fontSize: 12 }}>
          {dslManaged
            ? 'Schedule is managed by the Croniqfile. Adopt the job to edit it from the UI.'
            : `${schedules.length} schedule${schedules.length === 1 ? '' : 's'} attached.`}
        </span>
        <button
          type="button"
          className="btn sm"
          onClick={onAdd}
          disabled={dslManaged}
          title={dslManaged ? 'Adopt the job first to attach API schedules' : 'Attach a new schedule'}
        >
          <Plus size={13} /> Add schedule
        </button>
      </div>
      {schedules.length === 0 ? (
        <EmptyState
          icon={CalendarDays}
          title="No schedules"
          desc={
            dslManaged
              ? 'This job is managed via Croniqfile. Adopt it to attach API schedules.'
              : 'Attach a cron expression so the scheduler can fire this job.'
          }
        />
      ) : (
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
                <th style={{ width: 80 }}></th>
              </tr>
            </thead>
            <tbody>
              {schedules.map((s) => {
                const isDsl = s.managed_by === 'dsl'
                return (
                  <tr key={s.trigger_id}>
                    <td className="mono">{s.cron_expression ?? '—'}</td>
                    <td>{s.timezone ?? '—'}</td>
                    <td>{s.calendar ?? '—'}</td>
                    <td>{s.window ?? '—'}</td>
                    <td>
                      <span className={clsx('pill', isDsl ? 'outline' : 'accent')}>{s.managed_by}</span>
                    </td>
                    <td>
                      <StatusPill state={s.enabled ? 'enabled' : 'disabled'} />
                    </td>
                    <td className="dim">{formatRelative(s.updated_at)}</td>
                    <td>
                      <div className="row gap-6">
                        <button
                          type="button"
                          className="btn icon sm ghost"
                          aria-label="Edit schedule"
                          title={isDsl ? 'DSL-managed schedules are read-only' : 'Edit schedule'}
                          onClick={() => onEdit(s)}
                          disabled={isDsl}
                        >
                          <Pencil size={12} />
                        </button>
                        <button
                          type="button"
                          className="btn icon sm ghost danger-hover"
                          aria-label="Delete schedule"
                          title={isDsl ? 'DSL-managed schedules cannot be deleted via the UI' : 'Delete schedule'}
                          onClick={() => onDelete(s)}
                          disabled={isDsl}
                        >
                          <Trash2 size={12} />
                        </button>
                      </div>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </section>
      )}
    </div>
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

function AlertsTab({ jobKey }: { jobKey: string }) {
  // Per-job slice of the delivery log. The full filterable view lives on
  // the standalone Alerts page (linked from the sidebar) — here we only
  // show the rows that fired against THIS job_key so operators don't
  // have to switch pages while triaging a failing job.
  const { data, isLoading } = useAlertDeliveries({ job_key: jobKey, limit: 100 })
  return (
    <section className="card" style={{ padding: 0 }}>
      <header
        className="row between"
        style={{
          padding: '12px 16px',
          borderBottom: '1px solid var(--border)',
          alignItems: 'center',
        }}
      >
        <div className="row" style={{ gap: 8, alignItems: 'center' }}>
          <Bell size={14} />
          <p className="card-title" style={{ margin: 0 }}>
            Alert deliveries for this job
          </p>
        </div>
        <span className="dim mono" style={{ fontSize: 11.5 }}>
          {data?.length ?? 0} row{data?.length === 1 ? '' : 's'}
        </span>
      </header>
      <DeliveriesList rows={data ?? []} isLoading={isLoading} />
    </section>
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
