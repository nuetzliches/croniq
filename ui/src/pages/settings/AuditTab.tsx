import { useMemo, useState } from 'react'
import { useAuditEvents } from '@/api/hooks'
import { EmptyState } from '@/components/primitives'
import { formatRelative } from '@/lib/utils'

const TARGET_OPTIONS = ['', 'user', 'job', 'execution', 'invitation', 'api_client', 'pat']

export function AuditTab() {
  const [targetType, setTargetType] = useState('')
  const [action, setAction] = useState('')
  const events = useAuditEvents({
    target_type: targetType || undefined,
    action: action.trim() || undefined,
    limit: 100,
  })

  const totalShown = events.data?.length ?? 0

  const eventsData = events.data
  const grouped = useMemo(() => {
    const m = new Map<string, typeof eventsData>()
    for (const e of eventsData ?? []) {
      const key = e.created_at.slice(0, 10)
      const arr = m.get(key) ?? []
      arr.push(e)
      m.set(key, arr)
    }
    return Array.from(m.entries()).sort(([a], [b]) => (a < b ? 1 : -1))
  }, [eventsData])

  return (
    <div className="col" style={{ gap: 14 }}>
      <section className="card">
        <div className="card-head">
          <p className="card-title">Filters</p>
          <span className="dim" style={{ fontSize: 11.5 }}>
            {totalShown} events
          </span>
        </div>
        <div className="row" style={{ gap: 8, flexWrap: 'wrap' }}>
          <label className="col" style={{ gap: 4, minWidth: 160 }}>
            <span style={{ fontSize: 11.5, color: 'var(--fg-2)' }}>Target type</span>
            <select className="input" value={targetType} onChange={(e) => setTargetType(e.target.value)}>
              {TARGET_OPTIONS.map((opt) => (
                <option key={opt} value={opt}>
                  {opt || '(all)'}
                </option>
              ))}
            </select>
          </label>
          <label className="col" style={{ gap: 4, minWidth: 200, flex: 1 }}>
            <span style={{ fontSize: 11.5, color: 'var(--fg-2)' }}>Action contains</span>
            <input
              className="input"
              placeholder="e.g. auth.login_failed"
              value={action}
              onChange={(e) => setAction(e.target.value)}
            />
          </label>
        </div>
      </section>

      <section className="card" style={{ padding: 0 }}>
        {events.isLoading ? (
          <div className="dim center" style={{ padding: 40 }}>Loading…</div>
        ) : totalShown === 0 ? (
          <EmptyState title="No matching events" desc="Try widening the filters or check back later." />
        ) : (
          grouped.map(([day, list]) => (
            <div key={day}>
              <div
                style={{
                  padding: '10px 16px 6px',
                  fontFamily: 'var(--font-mono-app)',
                  fontSize: 11,
                  color: 'var(--fg-mute)',
                  textTransform: 'uppercase',
                  letterSpacing: '0.08em',
                  borderTop: '1px solid var(--divider)',
                }}
              >
                {day}
              </div>
              {(list ?? []).map((e) => (
                <div
                  key={e.event_id}
                  className="row"
                  style={{
                    padding: '10px 16px',
                    borderTop: '1px solid var(--divider)',
                    gap: 12,
                    fontSize: 12.5,
                  }}
                >
                  <span className="dim mono tnum" style={{ width: 70, flexShrink: 0, fontSize: 11 }}>
                    {e.created_at.slice(11, 19)}
                  </span>
                  <span className="mono" style={{ minWidth: 160 }}>{e.action}</span>
                  <span className="dim grow ellipsis">
                    {e.target_type}
                    {e.target_id ? ` · ${e.target_id.slice(0, 8)}` : ''}
                  </span>
                  <span className="dim mono" style={{ fontSize: 11 }}>
                    {e.actor_type}
                    {e.actor_id ? ` · ${e.actor_id.slice(0, 8)}` : ''}
                  </span>
                  <span className="dim" style={{ fontSize: 11, minWidth: 70, textAlign: 'right' }}>
                    {formatRelative(e.created_at)}
                  </span>
                </div>
              ))}
            </div>
          ))
        )}
      </section>
    </div>
  )
}
