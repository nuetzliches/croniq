import { Link } from 'react-router'
import { useRef, useEffect } from 'react'
import { BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer, Cell } from 'recharts'
import { Activity, Cpu, Layers, TriangleAlert, CheckCircle } from 'lucide-react'
import { useHealth, useExecutions, useDeadLetters, useForecast } from '@/api/hooks'
import { StatCard } from '@/components/ui/stat-card'
import { Badge, stateVariant } from '@/components/ui/badge'
import { Card, CardHeader, CardTitle, CardContent } from '@/components/ui/card'
import { Spinner } from '@/components/ui/spinner'
import { EmptyState } from '@/components/ui/empty-state'
import { formatDate } from '@/lib/utils'

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
          sub={h?.runners_stale ? `${h.runners_stale} stale` : 'all healthy'}
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
              {executions.data?.length === 0 && (
                <EmptyState icon={<Activity className="h-8 w-8" />} title="No executions yet" />
              )}
              {executions.data?.map((e) => (
                <div key={e.id} className="flex items-center gap-3 px-4 py-2 text-xs hover:bg-accent/30 transition-colors">
                  <Badge variant={stateVariant(e.state)} className="shrink-0 w-20 justify-center">{e.state}</Badge>
                  <span className="font-mono text-foreground truncate flex-1">{e.job_key}</span>
                  {e.duration_ms && <span className="text-muted-foreground shrink-0">{e.duration_ms}ms</span>}
                  <span className="text-muted-foreground shrink-0">{formatDate(e.fire_at)}</span>
                </div>
              ))}
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Jobs stat mini-row */}
      <div className="grid grid-cols-3 gap-4">
        <StatCard label="Runners Stale" value={h?.runners_stale ?? '—'} icon={<Layers className="h-4 w-4" />} />
        <StatCard label="Runners Dead" value={h?.runners_dead ?? '—'} icon={<Cpu className="h-4 w-4" />} />
        <StatCard label="Server" value={<span className={h?.status === 'ok' ? 'text-status-ok-fg' : 'text-status-err-fg'}>{h?.status ?? '—'}</span>} icon={<CheckCircle className="h-4 w-4" />} />
      </div>
    </div>
  )
}
