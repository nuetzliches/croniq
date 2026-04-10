import { useState } from 'react'
import { useSchedules, useCreateSchedule, useDeleteSchedule } from '@/api/hooks'

export function SchedulesPage() {
  const schedules = useSchedules()
  const createSchedule = useCreateSchedule()
  const deleteSchedule = useDeleteSchedule()
  const [showCreate, setShowCreate] = useState(false)
  const [jobKey, setJobKey] = useState('')
  const [cron, setCron] = useState('')
  const [tz, setTz] = useState('')

  async function handleCreate(e: React.FormEvent) {
    e.preventDefault()
    await createSchedule.mutateAsync({ job_key: jobKey, cron_expression: cron, timezone: tz || undefined })
    setJobKey(''); setCron(''); setTz(''); setShowCreate(false)
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold">Schedules</h1>
        <button onClick={() => setShowCreate(!showCreate)} className="px-3 py-1.5 bg-primary text-primary-foreground rounded-md text-sm">
          {showCreate ? 'Cancel' : 'Create Schedule'}
        </button>
      </div>

      {showCreate && (
        <form onSubmit={handleCreate} className="bg-card border border-border rounded-lg p-4 space-y-3">
          <input placeholder="Job key" value={jobKey} onChange={(e) => setJobKey(e.target.value)} required className="w-full px-3 py-2 border border-border rounded-md text-sm" />
          <input placeholder="Cron expression (e.g. */15 * * * *)" value={cron} onChange={(e) => setCron(e.target.value)} required className="w-full px-3 py-2 border border-border rounded-md text-sm" />
          <input placeholder="Timezone (optional, e.g. Europe/Vienna)" value={tz} onChange={(e) => setTz(e.target.value)} className="w-full px-3 py-2 border border-border rounded-md text-sm" />
          <button type="submit" className="px-4 py-2 bg-primary text-primary-foreground rounded-md text-sm">Create</button>
        </form>
      )}

      <div className="bg-card border border-border rounded-lg overflow-hidden">
        <table className="w-full text-sm">
          <thead className="bg-muted">
            <tr>
              <th className="text-left px-3 py-2 font-medium">Job Key</th>
              <th className="text-left px-3 py-2 font-medium">Cron</th>
              <th className="text-left px-3 py-2 font-medium">Timezone</th>
              <th className="text-left px-3 py-2 font-medium">Enabled</th>
              <th className="text-left px-3 py-2 font-medium">Managed By</th>
              <th className="px-3 py-2"></th>
            </tr>
          </thead>
          <tbody>
            {schedules.data?.map((s) => (
              <tr key={s.trigger_id} className="border-t border-border">
                <td className="px-3 py-2 font-mono text-xs">{s.job_key}</td>
                <td className="px-3 py-2 font-mono text-xs">{s.cron_expression || '-'}</td>
                <td className="px-3 py-2">{s.timezone || 'UTC'}</td>
                <td className="px-3 py-2">{s.enabled ? <span className="text-green-600">Yes</span> : <span className="text-red-600">No</span>}</td>
                <td className="px-3 py-2 text-muted-foreground">{s.managed_by}</td>
                <td className="px-3 py-2 text-right">
                  <button onClick={() => deleteSchedule.mutate(s.trigger_id)} className="text-xs text-destructive hover:underline">Delete</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}
