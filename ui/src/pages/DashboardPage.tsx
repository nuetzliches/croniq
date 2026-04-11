import { useHealth, useJobs, useExecutions, useDeadLetters } from '@/api/hooks'

function StatCard({ label, value, color }: { label: string; value: string | number; color?: string }) {
  return (
    <div className="bg-card border border-border rounded-lg p-4">
      <p className="text-sm text-muted-foreground">{label}</p>
      <p className={`text-2xl font-semibold mt-1 ${color || ''}`}>{value}</p>
    </div>
  )
}

export function DashboardPage() {
  const health = useHealth()
  const jobs = useJobs()
  const executions = useExecutions({ limit: 10 })
  const deadLetters = useDeadLetters()

  const h = health.data

  return (
    <div className="space-y-6">
      <h1 className="text-lg font-semibold">Dashboard</h1>

      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <StatCard label="Queue Depth" value={h?.queued ?? '-'} />
        <StatCard label="Runners Online" value={h?.runners_online ?? '-'} color="text-green-600" />
        <StatCard label="Runners Stale" value={h?.runners_stale ?? '-'} color="text-yellow-600" />
        <StatCard label="Runners Dead" value={h?.runners_dead ?? '-'} color="text-red-600" />
        <StatCard label="Jobs" value={jobs.data?.length ?? '-'} />
        <StatCard label="Dead Letters" value={deadLetters.data?.length ?? '-'} color={deadLetters.data?.length ? 'text-red-600' : ''} />
      </div>

      <div>
        <h2 className="text-sm font-medium mb-2">Recent Executions</h2>
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
                      e.state === 'completed' ? 'bg-green-100 text-green-700' :
                      e.state === 'failed' ? 'bg-red-100 text-red-700' :
                      e.state === 'queued' ? 'bg-blue-100 text-blue-700' :
                      'bg-gray-100 text-gray-700'
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
