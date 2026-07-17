import { Badge } from '@/components/ui/badge'
import { stateVariant } from '@/components/ui/badge-variants'
import { CopyButton } from '@/components/ui/copy-button'
import { LogsPanel } from '@/components/LogsPanel'
import { JobLink, RunnerLink } from '@/components/entity-links'
import { RelativeTime } from '@/components/ui/relative-time'
import type { Execution } from '@/api/types'
import { formatDate, isRescheduled } from '@/lib/utils'

export function ExecutionDetail({ execution }: { execution: Execution }) {
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
          ['Job', <JobLink key="job" jobKey={execution.job_key} className="font-mono" />],
          ['State', <Badge key="s" variant={stateVariant(execution.state)}>{execution.state}</Badge>],
          ['Attempt', execution.attempt],
          ['Runner', execution.runner_id ? <RunnerLink key="runner" runnerId={execution.runner_id} className="font-mono" /> : '—'],
          ['Duration', execution.duration_ms ? `${execution.duration_ms}ms` : '—'],
          ['Fire at', formatDate(execution.fire_at)],
          // Original logical fire time — only shown when it diverges from
          // fire_at (retry or dead-letter replay); redundant otherwise.
          ...(isRescheduled(execution)
            ? [[
                'Scheduled for',
                <span key="sf" className="flex flex-wrap items-baseline gap-1.5">
                  {formatDate(execution.scheduled_for)}
                  <span className="text-muted-foreground">·</span>
                  <RelativeTime iso={execution.scheduled_for} />
                </span>,
              ]]
            : []),
          ['Completed', execution.completed_at ? formatDate(execution.completed_at) : '—'],
          // Only present on triggered executions whose caller sent a dedup
          // key (#279) — hidden for the (vast) keyless majority.
          ...(execution.idempotency_key
            ? [[
                'Idempotency key',
                <span key="ik" className="flex items-center gap-1.5 font-mono">
                  {execution.idempotency_key}
                  <CopyButton value={execution.idempotency_key} label="Copy idempotency key" />
                </span>,
              ]]
            : []),
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

      <LogsPanel executionId={execution.id} executionState={execution.state} />
    </div>
  )
}
