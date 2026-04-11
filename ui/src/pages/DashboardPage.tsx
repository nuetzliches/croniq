import { Link } from 'react-router'
import { useHealth, useJobs, useExecutions, useDeadLetters, useForecast } from '@/api/hooks'

function StatCard({ label, value, color, href }: { label: string; value: string | number; color?: string; href?: string }) {
  const inner = (
    <div className="bg-card border border-border rounded-lg p-4">
      <p className="text-sm text-muted-foreground">{label}</p>
      <p className={`text-2xl font-semibold mt-1 ${color || ''}`}>{value}</p>
    </div>
  )
  return href ? <Link to={href} className="block hover:opacity-80 transition-opacity">{inner}</Link> : inner
}

export function DashboardPage() {
  const health = useHealth()
  const jobs = useJobs()
  const executions = useExecutions({ limit: 10 })
  const execStats = useExecutions({ limit: 50 })
  const deadLetters = useDeadLetters()
  const forecast = useForecast(120)

  const h = health.data

  const terminal = execStats.data?.filter(e => ['completed', 'failed', 'dead'].includes(e.state)) ?? []
  const successRate = terminal.length > 0
    ? Math.round(terminal.filter(e => e.state === 'completed').length / terminal.length * 100)
    : null
  const successColor = successRate === null ? '' : successRate >= 95 ? 'text-status-ok-fg' : successRate >= 80 ? 'text-status-warn-fg' : 'text-status-err-fg'

  const dlCount = deadLetters.data?.length ?? 0
  const upcomingBuckets = forecast.data?.buckets.filter(b => b.count > 0).slice(0, 8) ?? []

  return (
    <div className="space-y-6">
      <h1 className="text-lg font-semibold">Dashboard</h1>

      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <StatCard label="Queue Depth" value={h?.queued ?? '-'} />
        <StatCard label="Runners Online" value={h?.runners_online ?? '-'} color="text-status-ok-fg" />
        <StatCard label="Runners Stale" value={h?.runners_stale ?? '-'} color="text-status-warn-fg" />
        <StatCard label="Runners Dead" value={h?.runners_dead ?? '-'} color="text-status-err-fg" />
        <StatCard label="Jobs" value={jobs.data?.length ?? '-'} />
        <StatCard
          label="Dead Letters"
          value={dlCount || '-'}
          color={dlCount > 0 ? 'text-status-err-fg' : ''}
          href={dlCount > 0 ? '/dead-letters' : undefined}
        />
        <StatCard
          label="Success Rate"
          value={successRate !== null ? `${successRate}%` : '-'}
          color={successColor}
        />
      </div>

      <div>
        <h2 className="text-sm font-medium mb-2">Upcoming (next 2h)</h2>
        <div className="bg-card border border-border rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-muted">
              <tr>
                <th className="text-left px-3 py-2 font-medium">Time</th>
                <th className="text-left px-3 py-2 font-medium">Count</th>
                <th className="text-left px-3 py-2 font-medium">Jobs</th>
              </tr>
            </thead>
            <tbody>
              {upcomingBuckets.map((b, i) => (
                <tr key={i} className="border-t border-border">
                  <td className="px-3 py-2 text-muted-foreground">{new Date(b.start).toLocaleTimeString()}</td>
                  <td className="px-3 py-2 font-semibold">{b.count}</td>
                  <td className="px-3 py-2 font-mono text-xs text-muted-foreground">
                    {b.jobs.slice(0, 3).join(', ')}{b.jobs.length > 3 ? ` +${b.jobs.length - 3}` : ''}
                  </td>
                </tr>
              ))}
              {upcomingBuckets.length === 0 && (
                <tr><td colSpan={3} className="px-3 py-4 text-center text-muted-foreground">No upcoming triggers</td></tr>
              )}
            </tbody>
          </table>
        </div>
      </div>

      <div>
        <div className="flex items-center justify-between mb-2">
          <h2 className="text-sm font-medium">Recent Executions</h2>
          <Link to="/executions" className="text-xs text-primary hover:underline">View all →</Link>
        </div>
        <div className="bg-card border border-border rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-muted">
              <tr>
                <th className="text-left px-3 py-2 font-medium">Job</th>
                <th className="text-left px-3 py-2 font-medium">State</th>
                <th className="text-left px-3 py-2 font-medium">Runner</th>
                <th className="text-left px-3 py-2 font-medium">Fire At</th>
              </tr>
            </thead>
            <tbody>
              {executions.data?.map((e) => (
                <tr key={e.id} className="border-t border-border">
                  <td className="px-3 py-2 font-mono text-xs">{e.job_key}</td>
                  <td className="px-3 py-2">
                    <span className={`px-2 py-0.5 rounded text-xs font-medium ${
                      e.state === 'completed' ? 'bg-status-ok-bg text-status-ok-fg' :
                      e.state === 'failed' ? 'bg-status-err-bg text-status-err-fg' :
                      e.state === 'queued' ? 'bg-status-info-bg text-status-info-fg' :
                      'bg-status-neutral-bg text-status-neutral-fg'
                    }`}>{e.state}</span>
                  </td>
                  <td className="px-3 py-2 text-muted-foreground">{e.runner_id || '-'}</td>
                  <td className="px-3 py-2 text-muted-foreground">{new Date(e.fire_at).toLocaleString()}</td>
                </tr>
              ))}
              {!executions.data?.length && (
                <tr><td colSpan={4} className="px-3 py-4 text-center text-muted-foreground">No executions yet</td></tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  )
}
