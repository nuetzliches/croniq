import { useState } from 'react'
import { useExecutions } from '@/api/hooks'
import { EmptyState, StatusPill } from '@/components/primitives'
import { Activity, X } from 'lucide-react'
import { shortId } from '@/lib/utils'
import { RelativeTime } from '@/components/ui/relative-time'

const STATES = ['', 'queued', 'claimed', 'completed', 'failed', 'dead', 'cancelled']
const PAGE_SIZE = 50

export function ExecutionsPage() {
  const [stateFilter, setStateFilter] = useState('')
  const [jobFilter, setJobFilter] = useState('')
  const [limit, setLimit] = useState(PAGE_SIZE)
  const executions = useExecutions({
    state: stateFilter || undefined,
    job_key: jobFilter || undefined,
    limit,
  })

  const rows = executions.data ?? []
  const hasFilters = !!(stateFilter || jobFilter)
  const reachedEnd = rows.length < limit

  function setStateAndReset(v: string) {
    setStateFilter(v)
    setLimit(PAGE_SIZE)
  }
  function setJobAndReset(v: string) {
    setJobFilter(v)
    setLimit(PAGE_SIZE)
  }

  return (
    <div className="page wide">
      <div className="page-head">
        <div>
          <h1 className="page-title">Executions</h1>
          <p className="page-subtitle">Filter by state or job to inspect recent executions.</p>
        </div>
        <span className="dim mono" style={{ fontSize: 12 }}>
          {rows.length} {rows.length === 1 ? 'row' : 'rows'}
          {!reachedEnd && ' (recent)'}
        </span>
      </div>

      <section className="card" style={{ padding: 0 }}>
        <div
          className="row"
          style={{
            padding: 12,
            gap: 8,
            flexWrap: 'wrap',
            borderBottom: rows.length > 0 ? '1px solid var(--divider)' : undefined,
          }}
        >
          <label className="row" style={{ gap: 6, fontSize: 12 }}>
            <span className="dim">State</span>
            <select className="input" value={stateFilter} onChange={(e) => setStateAndReset(e.target.value)} style={{ width: 160 }}>
              {STATES.map((s) => (
                <option key={s} value={s}>
                  {s || 'all'}
                </option>
              ))}
            </select>
          </label>
          <label className="row" style={{ gap: 6, fontSize: 12, flex: 1, minWidth: 200 }}>
            <span className="dim">Job key</span>
            <div style={{ position: 'relative', flex: 1 }}>
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
                  style={{ position: 'absolute', right: 2, top: 2, width: 28, height: 28 }}
                >
                  <X size={12} />
                </button>
              ) : null}
            </div>
          </label>
          {hasFilters ? (
            <button
              type="button"
              className="btn sm ghost"
              onClick={() => {
                setStateAndReset('')
                setJobAndReset('')
              }}
            >
              Clear filters
            </button>
          ) : null}
        </div>

        {executions.isLoading ? (
          <div className="dim center" style={{ padding: 40 }}>Loading…</div>
        ) : rows.length === 0 ? (
          <EmptyState
            icon={Activity}
            title="No executions"
            desc={hasFilters ? 'Nothing matches the current filters.' : 'Executions will appear here once jobs start firing.'}
          />
        ) : (
          <table className="tbl">
            <thead>
              <tr>
                <th>ID</th>
                <th>Job</th>
                <th>State</th>
                <th>Runner</th>
                <th>Duration</th>
                <th>Fire at</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((e) => (
                <tr key={e.id}>
                  <td className="mono dim" style={{ fontSize: 11.5 }} title={e.id}>
                    {shortId(e.id)}
                  </td>
                  <td className="mono">{e.job_key}</td>
                  <td>
                    <StatusPill state={e.state} />
                  </td>
                  <td className="mono dim ellipsis" style={{ maxWidth: 180, fontSize: 11.5 }}>
                    {e.runner_id ?? '—'}
                  </td>
                  <td className="num">{e.duration_ms ? `${e.duration_ms} ms` : '—'}</td>
                  <td className="dim">
                    <RelativeTime iso={e.fire_at} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      {rows.length >= limit ? (
        <div className="row center">
          <button type="button" className="btn sm ghost" onClick={() => setLimit((n) => n + PAGE_SIZE)}>
            Load {PAGE_SIZE} more
          </button>
        </div>
      ) : null}
    </div>
  )
}
