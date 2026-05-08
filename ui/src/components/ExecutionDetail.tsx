import { useExecutionLogs } from '@/api/hooks'
import { Badge } from '@/components/ui/badge'
import { stateVariant } from '@/components/ui/badge-variants'
import { CopyButton } from '@/components/ui/copy-button'
import { Spinner } from '@/components/ui/spinner'
import type { Execution } from '@/api/types'
import { formatDate } from '@/lib/utils'

function LogLine({ level, message }: { level: string; message: string }) {
  const color = level === 'error' ? 'text-status-err-fg' : level === 'warn' ? 'text-status-warn-fg' : 'text-muted-foreground'
  return (
    <div className="text-xs font-mono leading-5">
      <span className={color}>[{level}]</span>{' '}{message}
    </div>
  )
}

export function ExecutionDetail({ execution }: { execution: Execution }) {
  const logs = useExecutionLogs(execution.id)
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-xs">
        {[
          ['ID', (
            <span key="id" className="flex items-center gap-1.5 font-mono">
              {execution.id}
              <CopyButton value={execution.id} label="Copy execution id" />
            </span>
          )],
          ['Job', execution.job_key],
          ['State', <Badge key="s" variant={stateVariant(execution.state)}>{execution.state}</Badge>],
          ['Attempt', execution.attempt],
          ['Runner', execution.runner_id || '—'],
          ['Duration', execution.duration_ms ? `${execution.duration_ms}ms` : '—'],
          ['Fire at', formatDate(execution.fire_at)],
          ['Completed', execution.completed_at ? formatDate(execution.completed_at) : '—'],
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
