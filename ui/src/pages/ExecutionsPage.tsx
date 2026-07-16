import { useState } from 'react'
import { useNavigate, useParams, useSearchParams } from 'react-router'
import clsx from 'clsx'
import { useExecutions, useCancelExecution } from '@/api/hooks'
import type { Execution } from '@/api/types'
import { StatusPill } from '@/components/primitives'
import { EmptyState } from '@/components/ui/empty-state'
import { Activity, Ban, X, MousePointerClick } from 'lucide-react'
import { shortId } from '@/lib/utils'
import { RelativeTime } from '@/components/ui/relative-time'
import { ExecutionDetail } from '@/components/ExecutionDetail'
import { JobLink, RunnerLink } from '@/components/entity-links'

const STATES = ['', 'queued', 'claimed', 'completed', 'failed', 'dead', 'cancelled']
const PAGE_SIZE = 50

export function ExecutionsPage() {
  const navigate = useNavigate()
  const { id: routeId } = useParams<{ id: string }>()
  // Filters live in the URL (?state=…&job_key=…) so the view is
  // shareable and deep-linkable — e.g. the per-job "View all executions"
  // link points here pre-filtered to that job_key.
  const [searchParams, setSearchParams] = useSearchParams()
  const stateFilter = searchParams.get('state') ?? ''
  const jobFilter = searchParams.get('job_key') ?? ''
  const runnerFilter = searchParams.get('runner_id') ?? ''
  const [limit, setLimit] = useState(PAGE_SIZE)
  const executions = useExecutions({
    state: stateFilter || undefined,
    job_key: jobFilter || undefined,
    runner_id: runnerFilter || undefined,
    limit,
  })
  const cancelExecution = useCancelExecution()

  const rows = executions.data ?? []
  const hasFilters = !!(stateFilter || jobFilter || runnerFilter)
  const reachedEnd = rows.length < limit
  const selected = routeId ? rows.find((r) => r.id === routeId) ?? null : null

  // Preserve the active filters when navigating to / from the detail route.
  const selectExecution = (id: string | null) =>
    navigate({ pathname: id ? `/executions/${id}` : '/executions', search: searchParams.toString() })

  function setFilter(key: 'state' | 'job_key' | 'runner_id', v: string) {
    setSearchParams(
      (prev) => {
        const p = new URLSearchParams(prev)
        if (v) p.set(key, v)
        else p.delete(key)
        return p
      },
      { replace: true },
    )
    setLimit(PAGE_SIZE)
  }
  const setStateAndReset = (v: string) => setFilter('state', v)
  const setJobAndReset = (v: string) => setFilter('job_key', v)
  const setRunnerAndReset = (v: string) => setFilter('runner_id', v)

  return (
    <div className="split">
      <aside className="master" aria-label="Executions list">
        <div
          className="master-filter"
          style={{ padding: '12px 14px', display: 'flex', flexDirection: 'column', gap: 8 }}
        >
          <div className="row between">
            <span className="mono dim" style={{ fontSize: 12 }}>
              {rows.length} {rows.length === 1 ? 'row' : 'rows'}
              {!reachedEnd && ' (recent)'}
            </span>
          </div>
          <label className="col" style={{ gap: 4, fontSize: 12 }}>
            <span className="dim">State</span>
            <select
              className="input"
              value={stateFilter}
              onChange={(e) => setStateAndReset(e.target.value)}
            >
              {STATES.map((s) => (
                <option key={s} value={s}>
                  {s || 'all'}
                </option>
              ))}
            </select>
          </label>
          <label className="col" style={{ gap: 4, fontSize: 12 }}>
            <span className="dim">Job key</span>
            <div style={{ position: 'relative' }}>
              <input
                className="input"
                placeholder="substring match…"
                value={jobFilter}
                onChange={(e) => setJobAndReset(e.target.value)}
                style={{ paddingRight: 28 }}
              />
              {jobFilter ? (
                <button
                  type="button"
                  onClick={() => setJobAndReset('')}
                  aria-label="Clear filter"
                  className="btn icon sm ghost"
                  style={{ position: 'absolute', right: 2, top: 2, width: 24, height: 24 }}
                >
                  <X size={11} />
                </button>
              ) : null}
            </div>
          </label>
          {runnerFilter ? (
            <div className="col" style={{ gap: 4, fontSize: 12 }}>
              <span className="dim">Runner</span>
              <div className="row between" style={{ gap: 6 }}>
                <span className="mono ellipsis" style={{ minWidth: 0, flex: 1 }} title={runnerFilter}>
                  <RunnerLink runnerId={runnerFilter} />
                </span>
                <button
                  type="button"
                  onClick={() => setRunnerAndReset('')}
                  aria-label="Clear runner filter"
                  className="btn icon sm ghost"
                  style={{ width: 24, height: 24, flexShrink: 0 }}
                >
                  <X size={11} />
                </button>
              </div>
            </div>
          ) : null}
          {hasFilters ? (
            <button
              type="button"
              className="btn sm ghost"
              onClick={() => {
                setStateAndReset('')
                setJobAndReset('')
                setRunnerAndReset('')
              }}
            >
              Clear filters
            </button>
          ) : null}
        </div>

        <div className="master-list">
          {executions.isLoading ? (
            <div className="dim center" style={{ padding: 30 }}>Loading…</div>
          ) : rows.length === 0 ? (
            <EmptyState
              icon={<Activity className="h-10 w-10" />}
              title="No executions"
              description={hasFilters ? 'Nothing matches the current filters.' : 'Executions will appear here once jobs start firing.'}
            />
          ) : (
            <>
              {rows.map((e) => (
                <ExecutionRow
                  key={e.id}
                  execution={e}
                  active={e.id === routeId}
                  onClick={() => selectExecution(e.id)}
                  onCancel={() => cancelExecution.mutate(e.id)}
                  cancelDisabled={cancelExecution.isPending}
                />
              ))}
              {rows.length >= limit ? (
                <div className="row center" style={{ padding: 12 }}>
                  <button
                    type="button"
                    className="btn sm ghost"
                    onClick={() => setLimit((n) => n + PAGE_SIZE)}
                  >
                    Load {PAGE_SIZE} more
                  </button>
                </div>
              ) : null}
            </>
          )}
        </div>
      </aside>

      <section className="detail" aria-label="Execution detail">
        {selected ? (
          <div className="card" style={{ padding: '16px 20px' }}>
            <ExecutionDetail execution={selected} />
          </div>
        ) : routeId ? (
          <EmptyState
            icon={<Activity className="h-10 w-10" />}
            title="Execution not in current view"
            description={`No row matches ${routeId.slice(0, 8)}… in the loaded list. Clear filters or load more to find it.`}
          />
        ) : (
          <EmptyState
            icon={<MousePointerClick className="h-10 w-10" />}
            title="Select an execution"
            description="Pick a row on the left to see attempt details, the originating runner, error and logs."
          />
        )}
      </section>
    </div>
  )
}

function ExecutionRow({
  execution: e,
  active,
  onClick,
  onCancel,
  cancelDisabled,
}: {
  execution: Execution
  active: boolean
  onClick: () => void
  onCancel: () => void
  cancelDisabled: boolean
}) {
  const isCancellable = e.state === 'queued' || e.state === 'claimed'
  // The cancel control needs to be a real <button> for a11y + valid HTML
  // (nested <button> inside a parent <button> is invalid and trips screen
  // readers). So the row itself is a div with button semantics; pressing
  // Enter/Space activates it, just like a real button.
  return (
    <div
      role="button"
      tabIndex={0}
      className={clsx('job-row', active && 'active')}
      onClick={onClick}
      onKeyDown={(ev) => {
        if (ev.key === 'Enter' || ev.key === ' ') {
          ev.preventDefault()
          onClick()
        }
      }}
      style={{ cursor: 'pointer' }}
    >
      <div className="row between" style={{ gap: 8, alignItems: 'center' }}>
        <span className="key ellipsis mono" style={{ minWidth: 0, flex: 1, fontSize: 12 }}>
          <JobLink jobKey={e.job_key} />
        </span>
        <StatusPill state={e.state} />
        {isCancellable ? (
          <button
            type="button"
            className="btn icon sm ghost"
            title={e.state === 'claimed' ? 'Cancel running execution' : 'Cancel queued execution'}
            aria-label="Cancel execution"
            disabled={cancelDisabled}
            onClick={(ev) => {
              ev.stopPropagation()
              onCancel()
            }}
            style={{ flexShrink: 0 }}
          >
            <Ban size={11} />
          </button>
        ) : null}
      </div>
      <div className="row between" style={{ fontSize: 10.5 }}>
        <span className="mono dim" style={{ minWidth: 0, flex: 1 }} title={e.id}>
          {shortId(e.id)}
        </span>
        <span className="dim">
          <RelativeTime iso={e.fire_at} />
        </span>
      </div>
      <div className="row between" style={{ fontSize: 10.5 }}>
        <span className="mono dim ellipsis" style={{ minWidth: 0, flex: 1 }}>
          {e.runner_id ? <RunnerLink runnerId={e.runner_id} /> : '—'}
        </span>
        <span className="mono">{e.duration_ms ? `${e.duration_ms} ms` : '—'}</span>
      </div>
    </div>
  )
}
