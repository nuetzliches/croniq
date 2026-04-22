import { useState } from 'react'
import { useExecutions, useExecutionLogs } from '@/api/hooks'
import { Badge, stateVariant } from '@/components/ui/badge'
import { Sheet } from '@/components/ui/sheet'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import { List } from 'lucide-react'
import type { Execution } from '@/api/types'

const STATES = ['queued', 'claimed', 'completed', 'failed', 'dead', 'cancelled']

function LogLine({ level, message }: { level: string; message: string }) {
  const color = level === 'error' ? 'text-status-err-fg' : level === 'warn' ? 'text-status-warn-fg' : 'text-muted-foreground'
  return (
    <div className="text-xs font-mono leading-5">
      <span className={color}>[{level}]</span>{' '}{message}
    </div>
  )
}

function ExecutionDetail({ execution }: { execution: Execution }) {
  const logs = useExecutionLogs(execution.id)
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-xs">
        {[
          ['ID', execution.id.slice(0, 16) + '…'],
          ['Job', execution.job_key],
          ['State', <Badge key="s" variant={stateVariant(execution.state)}>{execution.state}</Badge>],
          ['Attempt', execution.attempt],
          ['Runner', execution.runner_id || '—'],
          ['Duration', execution.duration_ms ? `${execution.duration_ms}ms` : '—'],
          ['Fire at', new Date(execution.fire_at).toLocaleString()],
          ['Completed', execution.completed_at ? new Date(execution.completed_at).toLocaleString() : '—'],
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

      <div>
        <p className="text-xs text-muted-foreground mb-1">Logs</p>
        {logs.isLoading && <Spinner className="h-4 w-4" />}
        {logs.data?.length === 0 && <p className="text-xs text-muted-foreground">No logs for this execution</p>}
        <div className="bg-muted rounded-md p-3 max-h-64 overflow-auto space-y-0.5">
          {logs.data?.map((l) => <LogLine key={l.id} level={l.level} message={l.message} />)}
        </div>
      </div>
    </div>
  )
}

export function ExecutionsPage() {
  const [stateFilter, setStateFilter] = useState('')
  const [jobFilter, setJobFilter] = useState('')
  const [selected, setSelected] = useState<Execution | null>(null)
  const executions = useExecutions({ state: stateFilter || undefined, job_key: jobFilter || undefined, limit: 50 })

  const inputCls = 'px-3 py-1.5 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

  return (
    <div className="space-y-4">
      {/* Filter bar */}
      <div className="flex flex-wrap gap-2">
        <select value={stateFilter} onChange={(e) => setStateFilter(e.target.value)} className={inputCls}>
          <option value="">All states</option>
          {STATES.map(s => <option key={s} value={s}>{s}</option>)}
        </select>
        <input
          placeholder="Filter by job key…"
          value={jobFilter}
          onChange={(e) => setJobFilter(e.target.value)}
          className={`${inputCls} min-w-48`}
        />
        {(stateFilter || jobFilter) && (
          <button
            onClick={() => { setStateFilter(''); setJobFilter('') }}
            className="text-xs text-muted-foreground hover:text-foreground px-2"
          >
            Clear
          </button>
        )}
      </div>

      {executions.isLoading && <div className="flex justify-center py-12"><Spinner className="h-6 w-6" /></div>}

      {!executions.isLoading && executions.data?.length === 0 && (
        <EmptyState icon={<List className="h-10 w-10" />} title="No executions" description="Executions will appear here once jobs start running" />
      )}

      {(executions.data?.length ?? 0) > 0 && (
        <div className="rounded-lg border border-border bg-card overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border">
                {['ID', 'Job', 'State', 'Runner', 'Duration', 'Fire At'].map(h => (
                  <th key={h} className="px-3 py-2.5 text-left text-xs font-medium text-muted-foreground uppercase tracking-wide">{h}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {executions.data?.map((e) => (
                <tr
                  key={e.id}
                  onClick={() => setSelected(e)}
                  className={`border-b border-border last:border-0 cursor-pointer transition-colors hover:bg-accent/40 ${selected?.id === e.id ? 'bg-accent/60' : ''}`}
                >
                  <td className="px-3 py-2.5 font-mono text-xs text-muted-foreground">{e.id.slice(0, 8)}</td>
                  <td className="px-3 py-2.5 font-mono text-xs">{e.job_key}</td>
                  <td className="px-3 py-2.5"><Badge variant={stateVariant(e.state)}>{e.state}</Badge></td>
                  <td className="px-3 py-2.5 text-muted-foreground text-xs">{e.runner_id?.slice(0, 12) || '—'}</td>
                  <td className="px-3 py-2.5 text-muted-foreground">{e.duration_ms ? `${e.duration_ms}ms` : '—'}</td>
                  <td className="px-3 py-2.5 text-muted-foreground">{new Date(e.fire_at).toLocaleString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <Sheet open={!!selected} onOpenChange={(o) => !o && setSelected(null)} title="Execution Detail">
        {selected && <ExecutionDetail execution={selected} />}
      </Sheet>
    </div>
  )
}
