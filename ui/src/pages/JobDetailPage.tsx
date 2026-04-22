import { useParams } from 'react-router'
import { useJob, useSchedules, useExecutions } from '@/api/hooks'
import { Badge, stateVariant } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Spinner } from '@/components/ui/spinner'
import { formatDate } from '@/lib/utils'

export function JobDetailPage() {
  const { jobKey } = useParams<{ jobKey: string }>()
  const job = useJob(jobKey!)
  const schedules = useSchedules(jobKey)
  const executions = useExecutions({ job_key: jobKey, limit: 20 })

  if (job.isLoading) return <div className="flex justify-center py-12"><Spinner className="h-6 w-6" /></div>
  if (!job.data) return <p className="text-destructive text-sm">Job not found</p>

  const j = job.data

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-3">
        <span className="font-mono text-base font-semibold">{j.job_key}</span>
        <Badge variant={j.is_active ? 'ok' : 'neutral'}>{j.is_active ? 'active' : 'inactive'}</Badge>
      </div>

      <Card>
        <CardContent className="pt-4">
          <dl className="grid grid-cols-2 md:grid-cols-3 gap-x-6 gap-y-3 text-sm">
            {j.description && (
              <div className="col-span-full">
                <dt className="text-xs text-muted-foreground uppercase tracking-wide mb-0.5">Description</dt>
                <dd>{j.description}</dd>
              </div>
            )}
            <div>
              <dt className="text-xs text-muted-foreground uppercase tracking-wide mb-0.5">Runner</dt>
              <dd className="font-mono text-xs">{j.assigned_runner_id || '—'}</dd>
            </div>
            <div>
              <dt className="text-xs text-muted-foreground uppercase tracking-wide mb-0.5">Updated</dt>
              <dd>{formatDate(j.updated_at)}</dd>
            </div>
          </dl>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Schedules ({schedules.data?.length ?? 0})</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          {schedules.isLoading ? (
            <div className="flex justify-center py-6"><Spinner className="h-4 w-4" /></div>
          ) : (
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  {['Cron', 'Timezone', 'Enabled'].map((h) => (
                    <th key={h} className="px-3 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {schedules.data?.length === 0 && (
                  <tr><td colSpan={3} className="px-3 py-6 text-center text-sm text-muted-foreground">No schedules</td></tr>
                )}
                {schedules.data?.map((s) => (
                  <tr key={s.trigger_id} className="border-b border-border last:border-0 hover:bg-accent/30 transition-colors">
                    <td className="px-3 py-2.5 font-mono text-xs">{s.cron_expression || '—'}</td>
                    <td className="px-3 py-2.5 text-muted-foreground">{s.timezone || 'UTC'}</td>
                    <td className="px-3 py-2.5">
                      <Badge variant={s.enabled ? 'ok' : 'neutral'}>{s.enabled ? 'enabled' : 'disabled'}</Badge>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Recent Executions</CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          {executions.isLoading ? (
            <div className="flex justify-center py-6"><Spinner className="h-4 w-4" /></div>
          ) : (
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  {['ID', 'State', 'Fire At', 'Duration'].map((h) => (
                    <th key={h} className="px-3 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {executions.data?.length === 0 && (
                  <tr><td colSpan={4} className="px-3 py-6 text-center text-sm text-muted-foreground">No executions yet</td></tr>
                )}
                {executions.data?.map((e) => (
                  <tr key={e.id} className="border-b border-border last:border-0 hover:bg-accent/30 transition-colors">
                    <td className="px-3 py-2.5 font-mono text-xs text-muted-foreground">{e.id.slice(0, 8)}</td>
                    <td className="px-3 py-2.5">
                      <Badge variant={stateVariant(e.state)}>{e.state}</Badge>
                    </td>
                    <td className="px-3 py-2.5 text-muted-foreground">{formatDate(e.fire_at)}</td>
                    <td className="px-3 py-2.5 text-muted-foreground">{e.duration_ms ? `${e.duration_ms}ms` : '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
