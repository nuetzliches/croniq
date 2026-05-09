import { useState } from 'react'
import { Link } from 'react-router'
import { Trash2, Wifi, WifiOff } from 'lucide-react'
import { useRunnersSSE, useDeleteRunner, useJobs, useExecutions, useRunnerTags } from '@/api/hooks'
import { Badge } from '@/components/ui/badge'
import { stateVariant } from '@/components/ui/badge-variants'
import { Card, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { CopyButton } from '@/components/ui/copy-button'
import { RelativeTime } from '@/components/ui/relative-time'
import { useConfirm } from '@/components/ui/confirm-dialog'
import { Sheet } from '@/components/ui/sheet'
import { Spinner } from '@/components/ui/spinner'
import { LogsPanel } from '@/components/LogsPanel'
import type { RunnerSummary, Execution } from '@/api/types'
import { shortId, formatDate } from '@/lib/utils'

function CapacityRing({ inflight, max }: { inflight: number; max: number }) {
  const pct = max > 0 ? Math.min(inflight / max, 1) : 0
  const r = 20, cx = 26, cy = 26, circ = 2 * Math.PI * r
  const fill = circ * pct
  const color = pct < 0.6 ? 'var(--color-status-ok-fg)' : pct < 0.9 ? 'var(--color-status-warn-fg)' : 'var(--color-status-err-fg)'
  return (
    <svg width="52" height="52" aria-label={`${inflight} of ${max} inflight`} role="img">
      <circle cx={cx} cy={cy} r={r} fill="none" stroke="var(--color-border)" strokeWidth="4" />
      {max > 0 && (
        <circle cx={cx} cy={cy} r={r} fill="none" stroke={color} strokeWidth="4"
          strokeDasharray={`${fill} ${circ}`} strokeLinecap="round"
          transform={`rotate(-90 ${cx} ${cy})`} />
      )}
      <text x={cx} y={cy} textAnchor="middle" dominantBaseline="middle"
        fontSize="9" fill="currentColor" fontWeight="600">
        {inflight}/{max}
      </text>
    </svg>
  )
}

const statusVariant = (s: string) =>
  s === 'Online' ? 'ok' : s === 'Stale' ? 'warn' : 'err'

function ExecutionDetail({ execution }: { execution: Execution }) {
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-xs">
        {[
          ['ID', (
            <span key="id" className="flex items-center gap-1.5 font-mono">
              {execution.id}
              <CopyButton value={execution.id} label="Copy execution id" />
            </span>
          )],
          ['Job', execution.job_key],
          ['State', <Badge key="s" variant={stateVariant(execution.state)}>{execution.state}</Badge>],
          ['Attempt', execution.attempt],
          ['Runner', execution.runner_id || '—'],
          ['Duration', execution.duration_ms ? `${execution.duration_ms}ms` : '—'],
          ['Fire at', formatDate(execution.fire_at)],
          ['Completed', execution.completed_at ? formatDate(execution.completed_at) : '—'],
        ].map(([label, value]) => (
          <div key={String(label)} className="contents">
            <span className="text-muted-foreground">{label}</span>
            <span className="font-medium text-foreground">{value}</span>
          </div>
        ))}
      </div>

      {execution.error && (
        <div>
          <p className="text-xs text-muted-foreground mb-1">Error</p>
          <pre className="text-xs bg-muted rounded-md p-3 overflow-auto max-h-32 whitespace-pre-wrap">{execution.error}</pre>
        </div>
      )}

      <LogsPanel executionId={execution.id} executionState={execution.state} />
    </div>
  )
}

function RunnerDetail({ runner }: { runner: RunnerSummary }) {
  const [selectedExecution, setSelectedExecution] = useState<Execution | null>(null)
  const jobs = useJobs()
  const executions = useExecutions({ runner_id: runner.runner_id, limit: 50 })
  // Jobs this runner has actually handled recently — derived from execution
  // history. Croniq routes mostly by capability matching (not by pinning via
  // `assigned_runner_id`), so the explicit-assignment field is null for most
  // jobs even when a runner regularly executes them.
  const handledJobKeys = Array.from(new Set((executions.data ?? []).map((e) => e.job_key)))
  const handledJobs = handledJobKeys
    .map((key) => jobs.data?.find((j) => j.job_key === key) ?? { job_key: key })
    .sort((a, b) => a.job_key.localeCompare(b.job_key))

  return (
    <div className="space-y-6">
      {/* Identity */}
      <section className="space-y-3">
        <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Identity</h3>
        <div className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-xs items-start">
          <span className="text-muted-foreground pt-0.5">ID</span>
          <span className="flex items-center gap-1.5 font-mono break-all">
            {runner.runner_id}
            <CopyButton value={runner.runner_id} label="Copy runner id" />
          </span>

          <span className="text-muted-foreground pt-0.5">Status</span>
          <span><Badge variant={statusVariant(runner.status)}>{runner.status}</Badge></span>

          <span className="text-muted-foreground pt-0.5">Last poll</span>
          <span className="text-foreground"><RelativeTime iso={runner.last_poll_at} /></span>

          <span className="text-muted-foreground pt-0.5">Inflight</span>
          <span className="text-foreground">{runner.inflight} / {runner.max_inflight}</span>

          {runner.capabilities.length > 0 && (
            <>
              <span className="text-muted-foreground pt-0.5">Capabilities</span>
              <div className="flex flex-wrap gap-1">
                {runner.capabilities.map((c) => (
                  <span key={c} className="inline-flex items-center rounded-full bg-accent px-2 py-0.5 text-xs text-accent-foreground">
                    {c}
                  </span>
                ))}
              </div>
            </>
          )}

          {(runner.tags ?? []).length > 0 && (
            <>
              <span className="text-muted-foreground pt-0.5">Tags</span>
              <div className="flex flex-wrap gap-1">
                {(runner.tags ?? []).map((t) => (
                  <span key={t} className="inline-flex items-center rounded-full bg-accent px-2 py-0.5 text-xs font-mono text-accent-foreground">
                    {t}
                  </span>
                ))}
              </div>
            </>
          )}
        </div>
      </section>

      {/* Jobs handled (derived from recent executions) */}
      <section className="space-y-2">
        <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Jobs handled</h3>
        {executions.isLoading && <Spinner className="h-4 w-4" />}
        {!executions.isLoading && handledJobs.length === 0 && (
          <p className="text-xs text-muted-foreground">This runner hasn't handled any jobs recently</p>
        )}
        {handledJobs.length > 0 && (
          <ul className="space-y-1">
            {handledJobs.map((j) => (
              <li key={j.job_key}>
                <Link
                  to={`/jobs/${j.job_key}`}
                  className="text-xs font-mono text-primary hover:underline"
                >
                  {j.job_key}
                </Link>
              </li>
            ))}
          </ul>
        )}
      </section>

      {/* Recent executions */}
      <section className="space-y-2">
        <h3 className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Recent executions</h3>
        {executions.isLoading && <Spinner className="h-4 w-4" />}
        {!executions.isLoading && (executions.data?.length ?? 0) === 0 && (
          <p className="text-xs text-muted-foreground">No recent executions</p>
        )}
        {(executions.data?.length ?? 0) > 0 && (
          <div className="rounded-md border border-border overflow-hidden">
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b border-border">
                  {['ID', 'State', 'Fire At', 'Duration'].map((h) => (
                    <th key={h} className="px-2 py-2 text-left font-medium text-muted-foreground uppercase tracking-wide">{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {executions.data?.map((e) => (
                  <tr
                    key={e.id}
                    onClick={() => setSelectedExecution(e)}
                    className={`border-b border-border last:border-0 cursor-pointer transition-colors hover:bg-accent/40 ${selectedExecution?.id === e.id ? 'bg-accent/60' : ''}`}
                  >
                    <td className="px-2 py-2 font-mono text-muted-foreground" title={e.id}>{shortId(e.id)}</td>
                    <td className="px-2 py-2"><Badge variant={stateVariant(e.state)}>{e.state}</Badge></td>
                    <td className="px-2 py-2 text-muted-foreground"><RelativeTime iso={e.fire_at} /></td>
                    <td className="px-2 py-2 text-muted-foreground">{e.duration_ms ? `${e.duration_ms}ms` : '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <Sheet open={!!selectedExecution} onOpenChange={(o) => !o && setSelectedExecution(null)} title="Execution Detail">
        {selectedExecution && <ExecutionDetail execution={selectedExecution} />}
      </Sheet>
    </div>
  )
}

export function RunnersPage() {
  const { data: runners, isConnected } = useRunnersSSE()
  const tagCounts = useRunnerTags()
  const deleteRunner = useDeleteRunner()
  const { confirm, dialog: confirmDialog } = useConfirm()
  const [selectedRunner, setSelectedRunner] = useState<RunnerSummary | null>(null)
  const [activeTags, setActiveTags] = useState<Set<string>>(new Set())

  const toggleTag = (tag: string) =>
    setActiveTags((prev) => {
      const next = new Set(prev)
      if (next.has(tag)) next.delete(tag)
      else next.add(tag)
      return next
    })

  // AND-semantics: runner must carry every selected tag.
  const filteredRunners = (runners ?? []).filter((r) => {
    if (activeTags.size === 0) return true
    const have = new Set(r.tags ?? [])
    for (const t of activeTags) if (!have.has(t)) return false
    return true
  })

  async function handleDelete(e: React.MouseEvent, runnerId: string) {
    e.stopPropagation()
    const ok = await confirm({
      title: `Remove runner ${runnerId}?`,
      description:
        'In-flight executions belonging to this runner stay claimed until their lease expires, then time out. Use the runner shutdown signal for a graceful drain.',
      confirmLabel: 'Remove runner',
      destructive: true,
    })
    if (ok) deleteRunner.mutate(runnerId)
  }

  return (
    <div className="space-y-4">
      {confirmDialog}
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">{runners?.length ?? 0} runners</p>
        <span
          role="status"
          aria-live="polite"
          className="flex items-center gap-1.5 text-xs text-muted-foreground"
        >
          {isConnected
            ? <><Wifi className="h-3.5 w-3.5 text-status-ok-fg" /><span className="text-status-ok-fg">Live</span></>
            : <><WifiOff className="h-3.5 w-3.5" />Reconnecting…</>}
        </span>
      </div>

      {(tagCounts.data?.length ?? 0) > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-xs text-muted-foreground mr-1">Tags:</span>
          {tagCounts.data?.map((tc) => {
            const active = activeTags.has(tc.tag)
            return (
              <button
                key={tc.tag}
                type="button"
                onClick={() => toggleTag(tc.tag)}
                className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs transition-colors ${
                  active
                    ? 'bg-primary text-primary-foreground'
                    : 'bg-accent text-accent-foreground hover:bg-accent/70'
                }`}
                aria-pressed={active}
              >
                <span className="font-mono">{tc.tag}</span>
                <span className="opacity-70 tabular-nums">{tc.count}</span>
              </button>
            )
          })}
          {activeTags.size > 0 && (
            <button
              type="button"
              onClick={() => setActiveTags(new Set())}
              className="text-xs text-muted-foreground hover:text-foreground underline ml-1"
            >
              clear
            </button>
          )}
        </div>
      )}

      {!runners?.length && (
        <EmptyState
          icon={<Wifi className="h-10 w-10" />}
          title="No runners connected"
          description="Start a runner with the Runner SDK to see it here"
        />
      )}

      {(runners?.length ?? 0) > 0 && filteredRunners.length === 0 && (
        <p className="text-sm text-muted-foreground py-6 text-center">
          No runners match the selected tags.
        </p>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
        {filteredRunners.map((r) => (
          <Card
            key={r.runner_id}
            className="cursor-pointer transition-colors hover:bg-accent/40"
            onClick={() => setSelectedRunner(r)}
          >
            <CardContent className="pt-4">
              <div className="flex items-start justify-between gap-3">
                <div className="flex-1 min-w-0 space-y-2">
                  <div className="flex items-center gap-2">
                    <Badge variant={statusVariant(r.status)}>{r.status}</Badge>
                    <span className="font-mono text-xs text-muted-foreground truncate" title={r.runner_id}>{r.runner_id}</span>
                    <span onClick={(e) => e.stopPropagation()}>
                      <CopyButton value={r.runner_id} label={`Copy runner id ${r.runner_id}`} />
                    </span>
                  </div>

                  {r.capabilities.length > 0 && (
                    <div className="flex flex-wrap gap-1">
                      {r.capabilities.map((c) => (
                        <span key={c} className="inline-flex items-center rounded-full bg-accent px-2 py-0.5 text-xs text-accent-foreground">
                          {c}
                        </span>
                      ))}
                    </div>
                  )}

                  {(r.tags ?? []).length > 0 && (
                    <div className="flex flex-wrap gap-1">
                      {(r.tags ?? []).map((t) => (
                        <button
                          key={t}
                          type="button"
                          onClick={(e) => { e.stopPropagation(); toggleTag(t) }}
                          className="inline-flex items-center rounded-full bg-accent px-2 py-0.5 text-[10px] font-mono text-accent-foreground hover:bg-accent/70"
                          title={`Filter by ${t}`}
                        >
                          {t}
                        </button>
                      ))}
                    </div>
                  )}

                  <p className="text-xs text-muted-foreground">
                    Last poll <RelativeTime iso={r.last_poll_at} />
                  </p>
                </div>

                <div className="flex flex-col items-center gap-2 shrink-0">
                  <CapacityRing inflight={r.inflight} max={r.max_inflight} />
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={(e) => handleDelete(e, r.runner_id)}
                    aria-label={`Remove runner ${r.runner_id}`}
                    className="h-6 w-6 p-0 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </div>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      <Sheet
        open={!!selectedRunner}
        onOpenChange={(o) => !o && setSelectedRunner(null)}
        title="Runner Detail"
      >
        {selectedRunner && <RunnerDetail runner={selectedRunner} />}
      </Sheet>
    </div>
  )
}
