import { useState } from 'react'
import { Link } from 'react-router'
import { useJobs, useRegisterJob, useDeleteJob } from '@/api/hooks'

export function JobsPage() {
  const jobs = useJobs()
  const registerJob = useRegisterJob()
  const deleteJob = useDeleteJob()
  const [showCreate, setShowCreate] = useState(false)
  const [newKey, setNewKey] = useState('')
  const [newDesc, setNewDesc] = useState('')
  const [newSchedule, setNewSchedule] = useState('')
  const [newTz, setNewTz] = useState('')
  const [newTimeout, setNewTimeout] = useState('5m')

  async function handleCreate(e: React.FormEvent) {
    e.preventDefault()
    await registerJob.mutateAsync({
      job_key: newKey,
      schedule: newSchedule,
      timezone: newTz || undefined,
      timeout: newTimeout || undefined,
      description: newDesc || undefined,
    })
    setNewKey(''); setNewDesc(''); setNewSchedule(''); setNewTz(''); setNewTimeout('5m')
    setShowCreate(false)
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold">Jobs</h1>
        <button onClick={() => setShowCreate(!showCreate)} className="px-3 py-1.5 bg-primary text-primary-foreground rounded-md text-sm">
          {showCreate ? 'Cancel' : 'Create Job'}
        </button>
      </div>

      {showCreate && (
        <form onSubmit={handleCreate} className="bg-card border border-border rounded-lg p-4 space-y-3">
          <div className="grid grid-cols-2 gap-3">
            <input placeholder="Job key (e.g. billing:invoice)" value={newKey} onChange={(e) => setNewKey(e.target.value)} required className="col-span-2 px-3 py-2 border border-border rounded-md text-sm" />
            <input placeholder="Schedule (e.g. 5m, 1h, */15 * * * *)" value={newSchedule} onChange={(e) => setNewSchedule(e.target.value)} required className="px-3 py-2 border border-border rounded-md text-sm" />
            <input placeholder="Timeout (default: 5m)" value={newTimeout} onChange={(e) => setNewTimeout(e.target.value)} className="px-3 py-2 border border-border rounded-md text-sm" />
            <input placeholder="Timezone (e.g. Europe/Vienna)" value={newTz} onChange={(e) => setNewTz(e.target.value)} className="px-3 py-2 border border-border rounded-md text-sm" />
            <input placeholder="Description (optional)" value={newDesc} onChange={(e) => setNewDesc(e.target.value)} className="px-3 py-2 border border-border rounded-md text-sm" />
          </div>
          <button type="submit" disabled={registerJob.isPending} className="px-4 py-2 bg-primary text-primary-foreground rounded-md text-sm disabled:opacity-50">
            {registerJob.isPending ? 'Creating...' : 'Create & Schedule'}
          </button>
        </form>
      )}

      <div className="bg-card border border-border rounded-lg overflow-hidden">
        <table className="w-full text-sm">
          <thead className="bg-muted">
            <tr>
              <th className="text-left px-3 py-2 font-medium">Job Key</th>
              <th className="text-left px-3 py-2 font-medium">Description</th>
              <th className="text-left px-3 py-2 font-medium">Runner</th>
              <th className="text-left px-3 py-2 font-medium">Active</th>
              <th className="px-3 py-2"></th>
            </tr>
          </thead>
          <tbody>
            {jobs.data?.map((j) => (
              <tr key={j.job_key} className="border-t border-border">
                <td className="px-3 py-2"><Link to={`/jobs/${j.job_key}`} className="text-primary hover:underline font-mono text-xs">{j.job_key}</Link></td>
                <td className="px-3 py-2 text-muted-foreground">{j.description || '-'}</td>
                <td className="px-3 py-2 text-muted-foreground font-mono text-xs">{j.assigned_runner_id || '-'}</td>
                <td className="px-3 py-2">{j.is_active ? <span className="text-green-600">Yes</span> : <span className="text-red-600">No</span>}</td>
                <td className="px-3 py-2 text-right">
                  <button onClick={() => deleteJob.mutate(j.job_key)} className="text-xs text-destructive hover:underline">Delete</button>
                </td>
              </tr>
            ))}
            {!jobs.data?.length && (
              <tr><td colSpan={5} className="px-3 py-4 text-center text-muted-foreground">No jobs yet — create one above or register via Runner SDK</td></tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  )
}
