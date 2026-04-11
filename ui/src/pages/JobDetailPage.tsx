import { useParams } from 'react-router'
import { useJob, useSchedules, useExecutions } from '@/api/hooks'

export function JobDetailPage() {
  const { jobKey } = useParams<{ jobKey: string }>()
  const job = useJob(jobKey!)
  const schedules = useSchedules(jobKey)
  const executions = useExecutions({ job_key: jobKey, limit: 20 })

  if (job.isLoading) return <p className="text-muted-foreground">Loading...</p>
  if (!job.data) return <p className="text-destructive">Job not found</p>

  const j = job.data

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-lg font-semibold font-mono">{j.job_key}</h1>
        <p className="text-sm text-muted-foreground mt-1">{j.description || 'No description'}</p>
      </div>

      <div className="grid grid-cols-3 gap-4 text-sm">
        <div><span className="text-muted-foreground">Runner:</span> {j.assigned_runner_id || '-'}</div>
        <div><span className="text-muted-foreground">Active:</span> {j.is_active ? 'Yes' : 'No'}</div>
        <div><span className="text-muted-foreground">Updated:</span> {new Date(j.updated_at).toLocaleString()}</div>
      </div>

      <div>
        <h2 className="text-sm font-medium mb-2">Schedules ({schedules.data?.length ?? 0})</h2>
        <div className="bg-card border border-border rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-muted"><tr><th className="text-left px-3 py-2 font-medium">Cron</th><th className="text-left px-3 py-2 font-medium">Timezone</th><th className="text-left px-3 py-2 font-medium">Enabled</th></tr></thead>
            <tbody>
              {schedules.data?.map((s) => (
                <tr key={s.trigger_id} className="border-t border-border">
                  <td className="px-3 py-2 font-mono text-xs">{s.cron_expression || '-'}</td>
                  <td className="px-3 py-2">{s.timezone || 'UTC'}</td>
                  <td className="px-3 py-2">{s.enabled ? 'Yes' : 'No'}</td>
                </tr>
              ))}
              {!schedules.data?.length && <tr><td colSpan={3} className="px-3 py-4 text-center text-muted-foreground">No schedules</td></tr>}
            </tbody>
          </table>
        </div>
      </div>

      <div>
        <h2 className="text-sm font-medium mb-2">Recent Executions</h2>
        <div className="bg-card border border-border rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-muted"><tr><th className="text-left px-3 py-2 font-medium">ID</th><th className="text-left px-3 py-2 font-medium">State</th><th className="text-left px-3 py-2 font-medium">Fire At</th></tr></thead>
            <tbody>
              {executions.data?.map((e) => (
                <tr key={e.id} className="border-t border-border">
                  <td className="px-3 py-2 font-mono text-xs">{e.id.slice(0, 8)}</td>
                  <td className="px-3 py-2"><span className={`px-2 py-0.5 rounded text-xs font-medium ${e.state === 'completed' ? 'bg-status-ok-bg text-status-ok-fg' : e.state === 'failed' ? 'bg-status-err-bg text-status-err-fg' : 'bg-status-neutral-bg text-status-neutral-fg'}`}>{e.state}</span></td>
                  <td className="px-3 py-2 text-muted-foreground">{new Date(e.fire_at).toLocaleString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  )
}
