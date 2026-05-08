import { useState } from 'react'
import { useExecutions } from '@/api/hooks'
import { Badge } from '@/components/ui/badge'
import { stateVariant } from '@/components/ui/badge-variants'
import { Sheet } from '@/components/ui/sheet'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import { RelativeTime } from '@/components/ui/relative-time'
import { List, X } from 'lucide-react'
import type { Execution } from '@/api/types'
import { shortId } from '@/lib/utils'
import { ExecutionDetail } from '@/components/ExecutionDetail'

const PAGE_SIZE = 50

export function ExecutionsPage() {
  const [stateFilter, setStateFilter] = useState('')
  const [jobFilter, setJobFilter] = useState('')
  const [limit, setLimit] = useState(PAGE_SIZE)
  const [selected, setSelected] = useState<Execution | null>(null)
  const executions = useExecutions({ state: stateFilter || undefined, job_key: jobFilter || undefined, limit })

  const inputCls = 'px-3 py-1.5 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'
  const hasFilters = !!(stateFilter || jobFilter)
  const rows = executions.data ?? []
  const reachedEnd = rows.length < limit

  function setStateFilterAndReset(v: string) {
    setStateFilter(v)
    setLimit(PAGE_SIZE)
  }

  function setJobFilterAndReset(v: string) {
    setJobFilter(v)
    setLimit(PAGE_SIZE)
  }

  return (
    <div className="space-y-4">
      {/* Filter bar */}
      <div className="flex flex-wrap items-center gap-2">
        <select value={stateFilter} onChange={(e) => setStateFilterAndReset(e.target.value)} className={inputCls}>
          <option value="">All states</option>
          {STATES.map(s => <option key={s} value={s}>{s}</option>)}
        </select>
        {/* Inline clear inside the job-key input — clearer than the
            separate button it replaces, and matches the pattern users
            see on every search field elsewhere. */}
        <div className="relative">
          <input
            placeholder="Filter by job key (substring match)"
            value={jobFilter}
            onChange={(e) => setJobFilterAndReset(e.target.value)}
            className={`${inputCls} min-w-56 pr-7`}
          />
          {jobFilter && (
            <button
              onClick={() => setJobFilterAndReset('')}
              aria-label="Clear job filter"
              className="absolute right-1.5 top-1/2 -translate-y-1/2 p-0.5 text-muted-foreground hover:text-foreground"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
        {hasFilters && (
          <button
            onClick={() => { setStateFilterAndReset(''); setJobFilterAndReset('') }}
            className="text-xs text-muted-foreground hover:text-foreground px-2"
          >
            Clear all
          </button>
        )}
        <span className="text-xs text-muted-foreground ml-auto">
          {rows.length} {rows.length === 1 ? 'execution' : 'executions'}
          {!reachedEnd && ' (showing recent)'}
        </span>
      </div>

      {executions.isLoading && <div className="flex justify-center py-12"><Spinner className="h-6 w-6" /></div>}

      {!executions.isLoading && rows.length === 0 && (
        <EmptyState icon={<List className="h-10 w-10" />} title="No executions" description={hasFilters ? 'Nothing matches the current filters.' : 'Executions will appear here once jobs start running'} />
      )}

      {rows.length > 0 && (
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
              {rows.map((e) => (
                <tr
                  key={e.id}
                  onClick={() => setSelected(e)}
                  className={`border-b border-border last:border-0 cursor-pointer transition-colors hover:bg-accent/40 ${selected?.id === e.id ? 'bg-accent/60' : ''}`}
                >
                  <td className="px-3 py-2.5 font-mono text-xs text-muted-foreground" title={e.id}>{shortId(e.id)}</td>
                  <td className="px-3 py-2.5 font-mono text-xs">{e.job_key}</td>
                  <td className="px-3 py-2.5"><Badge variant={stateVariant(e.state)}>{e.state}</Badge></td>
                  <td className="px-3 py-2.5 text-muted-foreground font-mono text-xs">
                    <div className="max-w-[14rem] truncate" title={e.runner_id || undefined}>{e.runner_id || '—'}</div>
                  </td>
                  <td className="px-3 py-2.5 text-muted-foreground">{e.duration_ms ? `${e.duration_ms}ms` : '—'}</td>
                  <td className="px-3 py-2.5 text-muted-foreground">
                    <RelativeTime iso={e.fire_at} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Load-more pagination — bumps the limit by PAGE_SIZE and re-fetches.
          Hidden when fewer rows came back than were requested (we're at
          the tail of the data). Reset on filter change. */}
      {!executions.isLoading && rows.length >= limit && (
        <div className="flex justify-center">
          <button
            onClick={() => setLimit((n) => n + PAGE_SIZE)}
            className="text-xs text-primary hover:underline px-3 py-1.5"
          >
            Load {PAGE_SIZE} more
          </button>
        </div>
      )}

      <Sheet open={!!selected} onOpenChange={(o) => !o && setSelected(null)} title="Execution Detail">
        {selected && <ExecutionDetail execution={selected} />}
      </Sheet>
    </div>
  )
}
