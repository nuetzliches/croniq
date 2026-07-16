import { useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import clsx from 'clsx'
import { Trash2, Wifi, WifiOff, MousePointerClick, Activity, ArrowRight } from 'lucide-react'
import { useDeleteRunner, useRunnerTags, useExecutions } from '@/api/hooks'
import { useRunnersStream } from '@/api/runners-stream'
import type { RunnerSummary } from '@/api/types'
import { Badge } from '@/components/ui/badge'
import { stateVariant } from '@/components/ui/badge-variants'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { CopyButton } from '@/components/ui/copy-button'
import { RelativeTime } from '@/components/ui/relative-time'
import { Spinner } from '@/components/ui/spinner'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { JobLink } from '@/components/entity-links'

function CapacityRing({ inflight, max, size = 52 }: { inflight: number; max: number; size?: number }) {
  const pct = max > 0 ? Math.min(inflight / max, 1) : 0
  const cx = size / 2, cy = size / 2, r = size / 2 - 6
  const circ = 2 * Math.PI * r
  const fill = circ * pct
  const color = pct < 0.6 ? 'var(--color-status-ok-fg)' : pct < 0.9 ? 'var(--color-status-warn-fg)' : 'var(--color-status-err-fg)'
  return (
    <svg width={size} height={size} aria-label={`${inflight} of ${max} inflight`} role="img">
      <circle cx={cx} cy={cy} r={r} fill="none" stroke="var(--color-border)" strokeWidth="4" />
      {max > 0 && (
        <circle cx={cx} cy={cy} r={r} fill="none" stroke={color} strokeWidth="4"
          strokeDasharray={`${fill} ${circ}`} strokeLinecap="round"
          transform={`rotate(-90 ${cx} ${cy})`} />
      )}
      <text x={cx} y={cy} textAnchor="middle" dominantBaseline="middle"
        fontSize={size * 0.22} fill="currentColor" fontWeight="600">
        {inflight}/{max}
      </text>
    </svg>
  )
}

const statusVariant = (s: string) =>
  s === 'Online' ? 'ok' : s === 'Stale' ? 'warn' : 'err'

export function RunnersPage() {
  const { data: runners, isConnected } = useRunnersStream()
  const tagCounts = useRunnerTags()
  const navigate = useNavigate()
  const { runnerId: routeId } = useParams<{ runnerId: string }>()
  const [activeTags, setActiveTags] = useState<Set<string>>(new Set())

  const toggleTag = (tag: string) =>
    setActiveTags((prev) => {
      const next = new Set(prev)
      if (next.has(tag)) next.delete(tag)
      else next.add(tag)
      return next
    })

  const filteredRunners = (runners ?? []).filter((r) => {
    if (activeTags.size === 0) return true
    const have = new Set(r.tags ?? [])
    for (const t of activeTags) if (!have.has(t)) return false
    return true
  })

  const selected = (runners ?? []).find((r) => r.runner_id === routeId) ?? null
  const selectRunner = (id: string | null) =>
    navigate(id ? `/runners/${encodeURIComponent(id)}` : '/runners')

  return (
    <div className="split">
      <aside className="master" aria-label="Runners list">
        <div className="master-filter" style={{ padding: '12px 14px', display: 'flex', flexDirection: 'column', gap: 10 }}>
          <div className="row between" style={{ alignItems: 'center' }}>
            <span className="mono dim" style={{ fontSize: 12 }}>
              {runners?.length ?? 0} connected
            </span>
            <span
              role="status"
              aria-live="polite"
              className="flex items-center gap-1.5 text-xs text-muted-foreground"
            >
              {isConnected
                ? <><Wifi className="h-3 w-3 text-status-ok-fg" /><span className="text-status-ok-fg">Live</span></>
                : <><WifiOff className="h-3 w-3" />Reconnecting…</>}
            </span>
          </div>
          {(tagCounts.data?.length ?? 0) > 0 && (
            <div className="row gap-6" style={{ flexWrap: 'wrap' }}>
              <button
                type="button"
                className={clsx('pill', activeTags.size === 0 ? 'accent' : 'outline')}
                onClick={() => setActiveTags(new Set())}
              >
                All <span className="dim">{runners?.length ?? 0}</span>
              </button>
              {tagCounts.data?.slice(0, 8).map((tc) => (
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
          )}
        </div>

        <div className="master-list">
          {!runners ? (
            <div className="dim center" style={{ padding: 30 }}>Loading…</div>
          ) : runners.length === 0 ? (
            <EmptyState
              icon={<Wifi className="h-10 w-10" />}
              title="No runners connected"
              description="Start a runner with the Runner SDK to see it here"
            />
          ) : filteredRunners.length === 0 ? (
            <p className="text-sm text-muted-foreground py-6 text-center">
              No runners match the selected tags.
            </p>
          ) : (
            filteredRunners.map((r) => (
              <RunnerRow
                key={r.runner_id}
                runner={r}
                active={r.runner_id === selected?.runner_id}
                onClick={() => selectRunner(r.runner_id)}
              />
            ))
          )}
        </div>
      </aside>

      <section className="detail" aria-label="Runner detail">
        {selected ? (
          <RunnerDetail runner={selected} onClose={() => selectRunner(null)} />
        ) : (
          <EmptyState
            icon={<MousePointerClick className="h-10 w-10" />}
            title="Select a runner"
            description="Pick a runner on the left to see its capabilities, tags, capacity and recent executions."
          />
        )}
      </section>
    </div>
  )
}

function RunnerRow({
  runner,
  active,
  onClick,
}: {
  runner: RunnerSummary
  active: boolean
  onClick: () => void
}) {
  return (
    <button type="button" className={clsx('job-row', active && 'active')} onClick={onClick}>
      <div className="row between" style={{ gap: 8, alignItems: 'center' }}>
        <span className="row gap-6" style={{ minWidth: 0, flex: 1, alignItems: 'center' }}>
          <Badge variant={statusVariant(runner.status)}>{runner.status}</Badge>
          <span className="key ellipsis mono" style={{ fontSize: 12 }} title={runner.runner_id}>
            {runner.runner_id}
          </span>
        </span>
        <span className="mono dim tnum" style={{ fontSize: 11, flexShrink: 0 }}>
          {runner.inflight}/{runner.max_inflight}
        </span>
      </div>
      {(runner.tags ?? []).length > 0 && (
        <div className="meta ellipsis" style={{ minWidth: 0, fontSize: 11 }}>
          {(runner.tags ?? []).slice(0, 3).map((t) => (
            <span key={t} className="dim mono" style={{ marginRight: 6 }}>
              #{t}
            </span>
          ))}
        </div>
      )}
      <div className="row between" style={{ fontSize: 10.5 }}>
        <span className="dim mono">last poll <RelativeTime iso={runner.last_poll_at} /></span>
      </div>
    </button>
  )
}

function RunnerDetail({ runner, onClose }: { runner: RunnerSummary; onClose: () => void }) {
  const deleteRunner = useDeleteRunner()
  const { confirm, dialog: confirmDialog } = useConfirm()
  // 50 most-recent executions for this runner — server-side filter so we
  // don't ship the whole executions table to the client for an unselected
  // runner.
  const executions = useExecutions({ runner_id: runner.runner_id, limit: 50 })

  async function handleDelete() {
    const ok = await confirm({
      title: `Remove runner ${runner.runner_id}?`,
      description:
        'In-flight executions belonging to this runner stay claimed until their lease expires, then time out. Use the runner shutdown signal for a graceful drain.',
      confirmLabel: 'Remove runner',
      destructive: true,
    })
    if (ok) {
      await deleteRunner.mutateAsync(runner.runner_id)
      onClose()
    }
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 'var(--card-gap)' }}>
      {confirmDialog}
      <div className="card" style={{ padding: '16px 20px' }}>
        <div className="row between" style={{ marginBottom: 14, gap: 14, flexWrap: 'wrap', alignItems: 'flex-start' }}>
          <div className="col" style={{ gap: 6, minWidth: 0, flex: '1 1 280px' }}>
            <div className="row gap-8" style={{ flexWrap: 'wrap', alignItems: 'center' }}>
              <Badge variant={statusVariant(runner.status)}>{runner.status}</Badge>
              <h1 className="mono ellipsis" style={{ margin: 0, fontSize: 18, fontWeight: 600, color: 'var(--fg)' }} title={runner.runner_id}>
                {runner.runner_id}
              </h1>
              <CopyButton value={runner.runner_id} label={`Copy runner id ${runner.runner_id}`} />
            </div>
            <span className="dim" style={{ fontSize: 12 }}>
              Last poll <RelativeTime iso={runner.last_poll_at} />
            </span>
          </div>
          <div className="row gap-6" style={{ flexShrink: 0 }}>
            <Button
              variant="destructive"
              size="sm"
              onClick={handleDelete}
              disabled={deleteRunner.isPending}
            >
              <Trash2 className="h-3.5 w-3.5" /> Remove
            </Button>
          </div>
        </div>

        <div className="grid" style={{ gridTemplateColumns: 'auto 1fr', columnGap: 24, rowGap: 10, fontSize: 13, alignItems: 'center' }}>
          <span className="dim">Capacity</span>
          <div className="row gap-8" style={{ alignItems: 'center' }}>
            <CapacityRing inflight={runner.inflight} max={runner.max_inflight} size={42} />
            <span className="mono">{runner.inflight} / {runner.max_inflight} inflight</span>
          </div>

          <span className="dim">Capabilities</span>
          <div className="row gap-6" style={{ flexWrap: 'wrap' }}>
            {runner.capabilities.length === 0
              ? <span className="dim">—</span>
              : runner.capabilities.map((c) => (
                  <span key={c} className="pill outline mono">{c}</span>
                ))}
          </div>

          <span className="dim">Tags</span>
          <div className="row gap-6" style={{ flexWrap: 'wrap' }}>
            {(runner.tags ?? []).length === 0
              ? <span className="dim">—</span>
              : (runner.tags ?? []).map((t) => (
                  <span key={t} className="tag mono">{t}</span>
                ))}
          </div>
        </div>
      </div>

      <div className="card" style={{ padding: 0 }}>
        <div className="row between" style={{ padding: '12px 20px', borderBottom: '1px solid var(--border)', alignItems: 'center' }}>
          <p className="card-title" style={{ margin: 0 }}>Recent executions</p>
          <div className="row gap-8" style={{ alignItems: 'center' }}>
            <span className="dim mono" style={{ fontSize: 11 }}>
              {executions.data?.length ?? 0} rows
            </span>
            {(executions.data?.length ?? 0) > 0 ? (
              <Link
                to={`/executions?runner_id=${encodeURIComponent(runner.runner_id)}`}
                className="btn sm ghost"
                style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}
              >
                View all executions
                <ArrowRight size={12} />
              </Link>
            ) : null}
          </div>
        </div>
        {executions.isLoading ? (
          <div className="flex justify-center py-6"><Spinner className="h-4 w-4" /></div>
        ) : (executions.data?.length ?? 0) === 0 ? (
          <EmptyState
            icon={<Activity className="h-8 w-8" />}
            title="No recent executions"
            description="This runner has not claimed any executions yet."
          />
        ) : (
          <table className="tbl">
            <thead>
              <tr>
                <th>Job</th>
                <th>State</th>
                <th>Fired</th>
                <th className="num">Duration</th>
              </tr>
            </thead>
            <tbody>
              {(executions.data ?? []).map((e) => (
                <tr key={e.id}>
                  <td className="mono ellipsis" style={{ maxWidth: 220 }} title={e.job_key}><JobLink jobKey={e.job_key} /></td>
                  <td>
                    <Badge variant={stateVariant(e.state)}>{e.state}</Badge>
                  </td>
                  <td className="dim">
                    <RelativeTime iso={e.fire_at} />
                  </td>
                  <td className="num mono">
                    {e.duration_ms != null ? `${e.duration_ms} ms` : '—'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  )
}
