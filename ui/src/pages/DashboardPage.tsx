import { Link } from 'react-router'
import { Fragment, useRef, useEffect, useState } from 'react'
import { BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, Cell } from 'recharts'
import { Activity, Cpu, TriangleAlert, CheckCircle, Clock } from 'lucide-react'
import { useHealth, useExecutions, useDeadLetters, useForecast } from '@/api/hooks'
import { StatCard } from '@/components/ui/stat-card'
import { Badge } from '@/components/ui/badge'
import { stateVariant } from '@/components/ui/badge-variants'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'
import { Spinner } from '@/components/ui/spinner'
import { EmptyState } from '@/components/ui/empty-state'
import { RelativeTime } from '@/components/ui/relative-time'
import { formatRelative } from '@/lib/utils'

// If the most recent execution is older than this, the feed is treated
// as stale and a banner appears. Five minutes catches "all runners are
// down" without false-firing during a normal between-fire gap on a
// 1-min job.
const STALE_THRESHOLD_MS = 5 * 60_000

// Inline a divider in the feed when two adjacent rows are this far
// apart. Otherwise a multi-hour silence between two clusters reads as
// continuous activity ("12s ago, 4h ago, 5h ago…") with no visual
// break.
const GAP_THRESHOLD_MS = 30 * 60_000

/// Compact "X minutes / hours / days" label for a millisecond gap. Used
/// for the silence divider in the activity feed; deliberately coarser
/// than `formatRelative` so e.g. "12m 34s" rounds to "12m".
function formatGap(ms: number): string {
  const sec = Math.floor(ms / 1000)
  if (sec < 60) return `${sec}s`
  if (sec < 3600) return `${Math.floor(sec / 60)}m`
  if (sec < 86400) return `${Math.floor(sec / 3600)}h`
  return `${Math.floor(sec / 86400)}d`
}

function QueueGauge({ value }: { value: number }) {
  const max = Math.max(value, 10)
  const pct = Math.min(value / max, 1)
  const r = 28, cx = 36, cy = 36, circ = 2 * Math.PI * r
  const fill = circ * pct
  const color = value === 0 ? 'var(--color-status-ok-fg)' : value < 5 ? 'var(--color-status-warn-fg)' : 'var(--color-status-err-fg)'
  return (
    <svg width="72" height="72" aria-label={`Queue depth: ${value} jobs`} role="img">
      <circle cx={cx} cy={cy} r={r} fill="none" stroke="var(--color-border)" strokeWidth="5" />
      <circle cx={cx} cy={cy} r={r} fill="none" stroke={color} strokeWidth="5"
        strokeDasharray={`${fill} ${circ}`} strokeLinecap="round"
        transform={`rotate(-90 ${cx} ${cy})`} />
      <text x={cx} y={cy} textAnchor="middle" dominantBaseline="middle"
        fontSize="13" fontWeight="bold" fill="currentColor">{value}</text>
    </svg>
  )
}

export function DashboardPage() {
  const health = useHealth()
  const executions = useExecutions({ limit: 20 })
  const execStats = useExecutions({ limit: 100 })
  const deadLetters = useDeadLetters()
  const forecast = useForecast(120)
  const feedRef = useRef<HTMLDivElement>(null)

  const h = health.data
  const dlCount = deadLetters.data?.length ?? 0

  const terminal = execStats.data?.filter(e => ['completed', 'failed', 'dead'].includes(e.state)) ?? []
  const successRate = terminal.length > 0
    ? Math.round(terminal.filter(e => e.state === 'completed').length / terminal.length * 100)
    : null

  const forecastData = (forecast.data?.buckets ?? [])
    .filter(b => b.count > 0)
    .slice(0, 12)
    .map(b => ({
      time: new Date(b.start).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
      count: b.count,
    }))

  useEffect(() => {
    if (feedRef.current) {
      feedRef.current.scrollTop = 0
    }
  }, [executions.data])

  // Tick once a minute so the staleness banner re-evaluates as time
  // passes (otherwise a tab left open would never transition from "fresh"
  // to "stale" until the executions query refetches). Counter-based to
  // keep render pure under react-hooks/purity.
  const [nowTick, setNowTick] = useState(() => Date.now())
  useEffect(() => {
    const id = setInterval(() => setNowTick(Date.now()), 30_000)
    return () => clearInterval(id)
  }, [])
  const latestFireMs = executions.data?.[0]
    ? new Date(executions.data[0].fire_at).getTime()
    : null
  const stale =
    latestFireMs !== null && nowTick - latestFireMs > STALE_THRESHOLD_MS

  return (
    <div className="space-y-6">
      {/* Stats row */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
        <Card>
          <CardContent className="pt-4">
            <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide mb-2">Queue Depth</p>
            <QueueGauge value={h?.queued ?? 0} />
          </CardContent>
        </Card>

        <StatCard
          label="Runners Online"
          value={
            <span className="flex items-center gap-2">
              {h?.runners_online ?? '-'}
              {(h?.runners_online ?? 0) > 0 && (
                <span className="relative flex h-2 w-2">
                  <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-status-ok-fg opacity-75" />
                  <span className="relative inline-flex rounded-full h-2 w-2 bg-status-ok-fg" />
                </span>
              )}
            </span>
          }
          sub={[h?.runners_stale && `${h.runners_stale} stale`, h?.runners_dead && `${h.runners_dead} dead`].filter(Boolean).join(', ') || 'all healthy'}
          icon={<Cpu className="h-4 w-4" />}
        />

        <StatCard
          label="Success Rate"
          value={successRate !== null ? `${successRate}%` : '—'}
          sub={`${terminal.length} terminal executions`}
          icon={<CheckCircle className="h-4 w-4" />}
        />

        <StatCard
          label="Dead Letters"
          value={dlCount || '0'}
          sub={dlCount > 0 ? <Link to="/dead-letters" className="text-destructive hover:underline">View →</Link> : 'none pending'}
          icon={<TriangleAlert className={`h-4 w-4 ${dlCount > 0 ? 'text-destructive' : ''}`} />}
        />
      </div>

      {/* Charts row */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <Card>
          <CardHeader>
            <CardTitle>Upcoming Executions (2h)</CardTitle>
          </CardHeader>
          <CardContent>
            {forecast.isLoading ? (
              <div className="flex justify-center py-8"><Spinner className="h-5 w-5" /></div>
            ) : forecastData.length === 0 ? (
              <EmptyState title="No upcoming triggers" description="No jobs scheduled in the next 2 hours" />
            ) : (
              <ResponsiveContainer width="100%" height={160} aria-label="Forecast bar chart">
                <BarChart data={forecastData} margin={{ top: 4, right: 4, bottom: 0, left: -20 }}>
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--color-border)" />
                  <XAxis dataKey="time" tick={{ fontSize: 10, fill: 'var(--color-muted-foreground)' }} />
                  <YAxis tick={{ fontSize: 10, fill: 'var(--color-muted-foreground)' }} allowDecimals={false} />
                  <Tooltip
                    contentStyle={{ background: 'var(--color-card)', border: '1px solid var(--color-border)', borderRadius: 6, fontSize: 12 }}
                    labelStyle={{ color: 'var(--color-foreground)' }}
                  />
                  <Bar dataKey="count" radius={[3, 3, 0, 0]}>
                    {forecastData.map((_, i) => (
                      <Cell key={i} fill="var(--color-primary)" fillOpacity={0.8} />
                    ))}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            )}
          </CardContent>
        </Card>

        {/* Activity feed */}
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center justify-between">
              <span>Live Activity</span>
              <Link to="/executions" className="text-xs text-primary hover:underline font-normal">View all →</Link>
            </CardTitle>
          </CardHeader>
          <CardContent className="p-0">
            {/* Stale-data banner: if the most recent execution is more
                than STALE_THRESHOLD_MS old, surface that explicitly so
                a runner-offline situation doesn't read as "everything's
                fine, just quiet". Health-card runner counts catch the
                same condition in numbers; this lets a glance at the
                feed itself answer the question. */}
            {stale && executions.data?.[0] && (
              <div
                role="status"
                className="mx-4 mt-3 flex items-start gap-2 rounded-md border border-status-warn-fg/40 bg-status-warn-bg/40 px-3 py-2 text-xs text-status-warn-fg"
              >
                <Clock className="h-3.5 w-3.5 shrink-0 mt-0.5" aria-hidden="true" />
                <span>
                  No new executions for{' '}
                  <RelativeTime iso={executions.data[0].fire_at} />. Check{' '}
                  <Link to="/runners" className="underline hover:opacity-90">
                    runner health
                  </Link>
                  .
                </span>
              </div>
            )}
            <div
              ref={feedRef}
              role="feed"
              aria-label="Recent execution activity"
              aria-live="polite"
              className="max-h-48 overflow-y-auto divide-y divide-border"
            >
              {executions.isLoading && (
                <div className="flex justify-center py-6"><Spinner className="h-4 w-4" /></div>
              )}
              {!executions.isLoading && executions.data?.length === 0 && (
                // Compact placeholder — the full <EmptyState> component
                // adds 12+ rem of padding, which overshoots the 12rem
                // (max-h-48) panel and forces a scrollbar onto a totally
                // empty card. Two centered lines fit and read.
                <div className="flex flex-col items-center justify-center text-center py-6 px-4 text-muted-foreground">
                  <Activity className="h-6 w-6 mb-1.5" aria-hidden="true" />
                  <p className="text-xs">No executions yet</p>
                </div>
              )}
              {executions.data?.map((e, i) => {
                // Insert a gap divider above this row when the previous
                // row is more than GAP_THRESHOLD_MS more recent —
                // surfaces "system was idle for X" between two clusters
                // of activity instead of letting them visually merge.
                const prev = executions.data?.[i - 1]
                const prevAt = prev ? new Date(prev.fire_at).getTime() : null
                const thisAt = new Date(e.fire_at).getTime()
                const gap = prevAt !== null ? prevAt - thisAt : 0
                const showGap = gap > GAP_THRESHOLD_MS
                return (
                  <Fragment key={e.id}>
                    {showGap && (
                      <div
                        className="px-4 py-1.5 text-[10px] uppercase tracking-wide text-muted-foreground bg-muted/30 flex items-center gap-1.5"
                        role="separator"
                        aria-label={`Gap of ${formatRelative(new Date(thisAt + gap).toISOString(), thisAt)}`}
                      >
                        <span className="h-px flex-1 bg-border" />
                        <span>silence · {formatGap(gap)}</span>
                        <span className="h-px flex-1 bg-border" />
                      </div>
                    )}
                    <div className="flex items-center gap-3 px-4 py-2 text-xs hover:bg-accent/30 transition-colors">
                      <Badge variant={stateVariant(e.state)} className="shrink-0 w-20 justify-center">{e.state}</Badge>
                      <span className="font-mono text-foreground truncate flex-1">{e.job_key}</span>
                      {e.duration_ms && <span className="text-muted-foreground shrink-0">{e.duration_ms}ms</span>}
                      <span className="text-muted-foreground shrink-0"><RelativeTime iso={e.fire_at} /></span>
                    </div>
                  </Fragment>
                )
              })}
            </div>
          </CardContent>
        </Card>
      </div>

    </div>
  )
}
