import { useState } from 'react'
import { useExecutions, useExecutionLogs } from '@/api/hooks'

export function ExecutionsPage() {
  const [stateFilter, setStateFilter] = useState('')
  const [jobFilter, setJobFilter] = useState('')
  const executions = useExecutions({ state: stateFilter || undefined, job_key: jobFilter || undefined, limit: 50 })
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const logs = useExecutionLogs(selectedId || '')

  return (
    <div className="space-y-4">
      <h1 className="text-lg font-semibold">Executions</h1>

      <div className="flex gap-2">
        <select value={stateFilter} onChange={(e) => setStateFilter(e.target.value)} className="px-3 py-1.5 border border-border rounded-md text-sm bg-background">
          <option value="">All states</option>
          <option value="queued">Queued</option>
          <option value="claimed">Claimed</option>
          <option value="completed">Completed</option>
          <option value="failed">Failed</option>
          <option value="dead">Dead</option>
        </select>
        <input placeholder="Filter by job key" value={jobFilter} onChange={(e) => setJobFilter(e.target.value)} className="px-3 py-1.5 border border-border rounded-md text-sm bg-background text-foreground" />
      </div>

      <div className="flex gap-4">
        <div className="flex-1 bg-card border border-border rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead className="bg-muted">
              <tr>
                <th className="text-left px-3 py-2 font-medium">ID</th>
                <th className="text-left px-3 py-2 font-medium">Job</th>
                <th className="text-left px-3 py-2 font-medium">State</th>
                <th className="text-left px-3 py-2 font-medium">Runner</th>
                <th className="text-left px-3 py-2 font-medium">Duration</th>
              </tr>
            </thead>
            <tbody>
              {executions.data?.map((e) => (
                <tr key={e.id} className={`border-t border-border cursor-pointer hover:bg-muted ${selectedId === e.id ? 'bg-muted' : ''}`} onClick={() => setSelectedId(e.id)}>
                  <td className="px-3 py-2 font-mono text-xs">{e.id.slice(0, 8)}</td>
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
                  <td className="px-3 py-2 text-muted-foreground">{e.duration_ms ? `${e.duration_ms}ms` : '-'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        {selectedId && logs.data && (
          <div className="w-96 bg-card border border-border rounded-lg p-4 space-y-2">
            <h2 className="font-medium text-sm">Execution Logs</h2>
            {logs.data.length === 0 && <p className="text-xs text-muted-foreground">No logs</p>}
            <div className="space-y-1 max-h-96 overflow-auto">
              {logs.data.map((log) => (
                <div key={log.id} className="text-xs font-mono">
                  <span className={`${log.level === 'error' ? 'text-status-err-fg' : log.level === 'warn' ? 'text-status-warn-fg' : 'text-muted-foreground'}`}>
                    [{log.level}]
                  </span>{' '}
                  {log.message}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
