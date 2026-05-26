import { useEffect, useRef, useState, useMemo, useCallback } from 'react'
import { Terminal, Pause, Play, Trash2, Copy, Download, X } from 'lucide-react'
import { useAuthStore } from '@/auth/store'

// One event as emitted by GET /v1/events/stream. Keep in sync with
// `ConsoleEvent` in crates/croniq-server/src/live_console.rs.
interface LogEvent {
  ts: string
  level: 'trace' | 'debug' | 'info' | 'warn' | 'error' | string
  target: string
  message: string
  fields: Record<string, unknown>
}

const LEVELS: LogEvent['level'][] = ['debug', 'info', 'warn', 'error']
const MAX_BUFFER = 2000

function levelColor(level: string): string {
  switch (level) {
    case 'error':
      return 'var(--danger, #ef4444)'
    case 'warn':
      return 'var(--warn, #f59e0b)'
    case 'info':
      return 'var(--info, #3b82f6)'
    case 'debug':
      return 'var(--muted, #94a3b8)'
    default:
      return 'var(--text)'
  }
}

export function ConsolePage() {
  // Filters
  const [activeLevels, setActiveLevels] = useState<Set<string>>(
    new Set(['info', 'warn', 'error']),
  )
  const [search, setSearch] = useState('')
  const [paused, setPaused] = useState(false)
  const [events, setEvents] = useState<LogEvent[]>([])
  const [isConnected, setIsConnected] = useState(false)

  // Auto-scroll tracking — when the user manually scrolls up we stop
  // auto-following so they can read what they're looking at.
  const listRef = useRef<HTMLDivElement | null>(null)
  const stickyRef = useRef(true)
  const pendingRef = useRef<LogEvent[]>([])
  const pausedRef = useRef(paused)

  useEffect(() => {
    pausedRef.current = paused
  }, [paused])

  // SSE connection
  useEffect(() => {
    let stopped = false
    let ctrl = new AbortController()
    let backfilled = false
    const BASE = import.meta.env.VITE_API_URL ?? ''

    async function connect() {
      const token = useAuthStore.getState().token
      // Level filtering happens client-side (see `filtered`) so toggling a
      // level updates the view instantly instead of tearing down and
      // re-opening the stream — hence we subscribe to every level. Backfill
      // only on the first connect; reconnects pass snapshot=0 so a dropped
      // stream doesn't replay events already in the buffer.
      const url = `${BASE}/v1/events/stream${backfilled ? '?snapshot=0' : ''}`
      try {
        const res = await fetch(url, {
          signal: ctrl.signal,
          headers: {
            Accept: 'text/event-stream',
            ...(token ? { Authorization: `Bearer ${token}` } : {}),
          },
        })
        if (res.status === 401) {
          useAuthStore.getState().logout()
          return
        }
        if (res.status === 503) {
          // Server has no console hub (older binary, or tests). Don't
          // hammer it with reconnects.
          setIsConnected(false)
          return
        }
        if (!res.ok || !res.body) throw new Error(`SSE ${res.status}`)

        backfilled = true
        setIsConnected(true)
        const reader = res.body.getReader()
        const dec = new TextDecoder()
        let buf = ''
        while (true) {
          const { done, value } = await reader.read()
          if (done) break
          buf += dec.decode(value, { stream: true })
          const parts = buf.split('\n\n')
          buf = parts.pop() ?? ''
          for (const msg of parts) {
            const line = msg.split('\n').find((l) => l.startsWith('data:'))
            if (!line) continue
            try {
              const ev: LogEvent = JSON.parse(line.slice(5).trim())
              if (pausedRef.current) {
                pendingRef.current.push(ev)
                if (pendingRef.current.length > MAX_BUFFER) {
                  pendingRef.current.splice(0, pendingRef.current.length - MAX_BUFFER)
                }
              } else {
                setEvents((cur) => {
                  const next = cur.length >= MAX_BUFFER ? cur.slice(-MAX_BUFFER + 1) : cur.slice()
                  next.push(ev)
                  return next
                })
              }
            } catch {
              // skip malformed frame
            }
          }
        }
      } catch {
        /* will reconnect */
      } finally {
        setIsConnected(false)
      }

      if (!stopped) {
        setTimeout(() => {
          if (stopped) return
          ctrl = new AbortController()
          connect()
        }, 2000)
      }
    }

    connect()
    return () => {
      stopped = true
      ctrl.abort()
    }
  }, [])

  // When unpausing, flush buffered events.
  useEffect(() => {
    if (!paused && pendingRef.current.length > 0) {
      setEvents((cur) => {
        const merged = cur.concat(pendingRef.current)
        pendingRef.current = []
        return merged.length > MAX_BUFFER ? merged.slice(-MAX_BUFFER) : merged
      })
    }
  }, [paused])

  // Auto-scroll bottom when sticky + new events arrive.
  useEffect(() => {
    if (!stickyRef.current) return
    const el = listRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [events])

  const onScroll = useCallback(() => {
    const el = listRef.current
    if (!el) return
    // 32px tolerance — small scroll-up shouldn't lose autoscroll.
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 32
    stickyRef.current = atBottom
  }, [])

  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase()
    return events.filter((e) => {
      if (!activeLevels.has(e.level)) return false
      if (!needle) return true
      if (e.message.toLowerCase().includes(needle)) return true
      if (e.target.toLowerCase().includes(needle)) return true
      for (const v of Object.values(e.fields)) {
        if (String(v).toLowerCase().includes(needle)) return true
      }
      return false
    })
  }, [events, search, activeLevels])

  function toggleLevel(level: string) {
    setActiveLevels((cur) => {
      const next = new Set(cur)
      if (next.has(level)) next.delete(level)
      else next.add(level)
      return next
    })
  }

  function clearAll() {
    setEvents([])
    pendingRef.current = []
  }

  function copyAll() {
    const txt = filtered
      .map((e) => `${e.ts} ${e.level.toUpperCase().padEnd(5)} ${e.target} ${e.message}`)
      .join('\n')
    void navigator.clipboard.writeText(txt)
  }

  function downloadNdjson() {
    const ndjson = filtered.map((e) => JSON.stringify(e)).join('\n')
    const blob = new Blob([ndjson], { type: 'application/x-ndjson' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `croniq-console-${new Date().toISOString().replace(/[:.]/g, '-')}.ndjson`
    a.click()
    URL.revokeObjectURL(url)
  }

  return (
    <div className="page wide">
      <div className="page-head">
        <div>
          <h1 className="page-title">Live Console</h1>
          <p className="page-subtitle">
            Tail server tracing events in real time. {isConnected ? null : <span className="dim">(reconnecting…)</span>}
          </p>
        </div>
        <span className="dim mono" style={{ fontSize: 12 }}>
          {filtered.length} / {events.length} {events.length === 1 ? 'event' : 'events'}
          {paused ? ' (paused)' : ''}
        </span>
      </div>

      <section className="card" style={{ padding: 0 }}>
        <div
          className="row"
          style={{
            padding: 12,
            gap: 8,
            flexWrap: 'wrap',
            borderBottom: '1px solid var(--divider)',
          }}
        >
          <div className="row" style={{ gap: 6 }}>
            {LEVELS.map((lvl) => {
              const active = activeLevels.has(lvl)
              return (
                <button
                  key={lvl}
                  type="button"
                  className={`btn sm ${active ? '' : 'ghost'}`}
                  onClick={() => toggleLevel(lvl)}
                  title={`Toggle ${lvl}`}
                  style={{ textTransform: 'uppercase', letterSpacing: 0.5, fontSize: 11 }}
                >
                  <span style={{ color: levelColor(lvl) }}>●</span> {lvl}
                </button>
              )
            })}
          </div>
          <label className="row" style={{ gap: 6, fontSize: 12, flex: 1, minWidth: 220 }}>
            <span className="dim">Search</span>
            <div style={{ position: 'relative', flex: 1 }}>
              <input
                className="input"
                placeholder="message, target or field…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                style={{ paddingRight: 28 }}
              />
              {search ? (
                <button
                  type="button"
                  onClick={() => setSearch('')}
                  aria-label="Clear search"
                  className="btn icon sm ghost"
                  style={{ position: 'absolute', right: 2, top: 2, width: 28, height: 28 }}
                >
                  <X size={12} />
                </button>
              ) : null}
            </div>
          </label>
          <div className="row" style={{ gap: 4 }}>
            <button
              type="button"
              className="btn sm ghost"
              onClick={() => setPaused((p) => !p)}
              title={paused ? 'Resume' : 'Pause'}
            >
              {paused ? <Play size={12} /> : <Pause size={12} />}
              <span style={{ marginLeft: 4 }}>{paused ? 'Resume' : 'Pause'}</span>
            </button>
            <button type="button" className="btn icon sm ghost" onClick={clearAll} title="Clear">
              <Trash2 size={12} />
            </button>
            <button
              type="button"
              className="btn icon sm ghost"
              onClick={copyAll}
              title="Copy filtered events as text"
              disabled={filtered.length === 0}
            >
              <Copy size={12} />
            </button>
            <button
              type="button"
              className="btn icon sm ghost"
              onClick={downloadNdjson}
              title="Download filtered events as .ndjson"
              disabled={filtered.length === 0}
            >
              <Download size={12} />
            </button>
          </div>
        </div>

        <div
          ref={listRef}
          onScroll={onScroll}
          className="mono"
          style={{
            fontSize: 12,
            background: 'var(--surface-2, #0b0f17)',
            color: 'var(--text)',
            padding: 12,
            height: 'calc(100vh - 280px)',
            minHeight: 320,
            overflowY: 'auto',
            whiteSpace: 'pre-wrap',
            lineHeight: 1.5,
          }}
        >
          {filtered.length === 0 ? (
            <div className="dim center" style={{ padding: 40 }}>
              <Terminal size={20} style={{ marginBottom: 8 }} />
              <div>{events.length === 0 ? 'Waiting for server events…' : 'No events match the current filters.'}</div>
            </div>
          ) : (
            filtered.map((e, i) => (
              <div key={i} className="row" style={{ gap: 8, alignItems: 'flex-start' }}>
                <span className="dim" style={{ fontSize: 10, minWidth: 75 }}>
                  {e.ts.slice(11, 23)}
                </span>
                <span
                  style={{
                    color: levelColor(e.level),
                    fontWeight: 600,
                    minWidth: 50,
                    textTransform: 'uppercase',
                    fontSize: 10,
                  }}
                >
                  {e.level}
                </span>
                <span className="dim ellipsis" style={{ minWidth: 180, maxWidth: 240, fontSize: 10 }}>
                  {e.target}
                </span>
                <span style={{ flex: 1 }}>
                  {e.message}
                  {Object.keys(e.fields).length > 0 ? (
                    <span className="dim" style={{ marginLeft: 8, fontSize: 11 }}>
                      {Object.entries(e.fields)
                        .map(([k, v]) => `${k}=${typeof v === 'string' ? v : JSON.stringify(v)}`)
                        .join(' ')}
                    </span>
                  ) : null}
                </span>
              </div>
            ))
          )}
        </div>
      </section>
    </div>
  )
}
