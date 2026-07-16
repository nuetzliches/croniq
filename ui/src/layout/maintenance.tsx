import { useEffect, useRef, useState } from 'react'
import clsx from 'clsx'
import { Wrench, TriangleAlert } from 'lucide-react'
import { useMaintenance, useSetMaintenance } from '@/api/hooks'
import { Toggle } from '@/components/primitives'

// <input type="datetime-local"> works in the browser's local timezone; the API
// stores UTC (RFC3339). Convert on both edges.
function isoToLocalInput(iso: string | null | undefined): string {
  if (!iso) return ''
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`
}
function localInputToIso(v: string): string | null {
  if (!v) return null
  const d = new Date(v) // parsed as local time
  return Number.isNaN(d.getTime()) ? null : d.toISOString()
}

/**
 * Admin-only maintenance switch: a topbar button that opens a popover with the
 * manual toggle, an optional scheduled window, and a note. Setting it freezes
 * dispatch server-side. The read-only banner for all users is
 * [`MaintenanceBanner`].
 */
export function MaintenanceControl() {
  const { data } = useMaintenance()
  const setMaint = useSetMaintenance()
  const [open, setOpen] = useState(false)
  const [manual, setManual] = useState(false)
  const [start, setStart] = useState('')
  const [end, setEnd] = useState('')
  const [note, setNote] = useState('')
  const ref = useRef<HTMLDivElement>(null)

  const active = data?.active ?? false

  // Seed the form from the current server state each time the popover opens, so
  // an admin edits the live values rather than a stale snapshot.
  function openPopover() {
    setManual(data?.manual_active ?? false)
    setStart(isoToLocalInput(data?.window_start))
    setEnd(isoToLocalInput(data?.window_end))
    setNote(data?.note ?? '')
    setOpen(true)
  }

  useEffect(() => {
    if (!open) return
    function onDown(e: MouseEvent) {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  async function save() {
    await setMaint.mutateAsync({
      manual_active: manual,
      window_start: localInputToIso(start),
      window_end: localInputToIso(end),
      note: note.trim() || null,
    })
    setOpen(false)
  }

  async function disableAll() {
    await setMaint.mutateAsync({
      manual_active: false,
      window_start: null,
      window_end: null,
      note: null,
    })
    setOpen(false)
  }

  return (
    <div ref={ref} style={{ position: 'relative', display: 'inline-flex' }}>
      <button
        type="button"
        className="btn icon ghost"
        onClick={() => (open ? setOpen(false) : openPopover())}
        aria-label="Maintenance mode"
        aria-expanded={open}
        title={active ? 'Maintenance mode is ON' : 'Maintenance mode'}
        style={{ color: active ? 'var(--warn)' : undefined }}
      >
        <Wrench size={15} />
      </button>

      {open ? (
        <div
          className="card"
          role="dialog"
          aria-label="Maintenance mode settings"
          style={{
            position: 'absolute',
            top: 'calc(100% + 6px)',
            right: 0,
            width: 320,
            zIndex: 50,
            padding: 14,
            boxShadow: 'var(--shadow-lg)',
          }}
        >
          <div className="row between" style={{ marginBottom: 10 }}>
            <span className="card-title" style={{ margin: 0 }}>
              Maintenance mode
            </span>
            <span className={clsx('pill', active ? 'warn' : 'outline')} style={{ height: 18 }}>
              {active ? 'active' : 'off'}
            </span>
          </div>

          <p className="dim" style={{ fontSize: 11.5, margin: '0 0 12px', lineHeight: 1.5 }}>
            Freezes job dispatch: running jobs finish, scheduled fires are skipped, and
            queued work resumes when it ends. Manual triggers stay queued.
          </p>

          <label className="row between" style={{ marginBottom: 4, cursor: 'pointer' }}>
            <span style={{ fontSize: 13 }}>Pause now</span>
            <Toggle on={manual} onChange={setManual} label="Pause now" />
          </label>

          <div className="divider" style={{ margin: '10px 0' }} />

          <span
            className="dim"
            style={{ fontSize: 10.5, textTransform: 'uppercase', letterSpacing: '0.06em' }}
          >
            Scheduled window (optional)
          </span>

          <label className="col" style={{ gap: 4, fontSize: 12, marginTop: 8 }}>
            <span className="dim">Start</span>
            <input
              type="datetime-local"
              className="input"
              value={start}
              onChange={(e) => setStart(e.target.value)}
            />
          </label>
          <label className="col" style={{ gap: 4, fontSize: 12, marginTop: 8 }}>
            <span className="dim">End</span>
            <input
              type="datetime-local"
              className="input"
              value={end}
              onChange={(e) => setEnd(e.target.value)}
            />
          </label>
          <label className="col" style={{ gap: 4, fontSize: 12, marginTop: 8 }}>
            <span className="dim">Note (shown to everyone)</span>
            <input
              type="text"
              className="input"
              placeholder="e.g. DB migration in progress"
              value={note}
              maxLength={200}
              onChange={(e) => setNote(e.target.value)}
            />
          </label>

          <div className="row between" style={{ marginTop: 14, gap: 8 }}>
            <button
              type="button"
              className="btn sm ghost"
              onClick={disableAll}
              disabled={setMaint.isPending}
            >
              Disable
            </button>
            <button
              type="button"
              className="btn sm primary"
              onClick={save}
              disabled={setMaint.isPending}
            >
              {setMaint.isPending ? 'Saving…' : 'Save'}
            </button>
          </div>

          {setMaint.isError ? (
            <p style={{ color: 'var(--error)', fontSize: 11.5, margin: '8px 0 0' }}>
              Failed to update — check your permissions.
            </p>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

/** App-wide read-only banner shown to ALL users while maintenance is active. */
export function MaintenanceBanner() {
  const { data } = useMaintenance()
  if (!data?.active) return null
  const until = data.window_end ? new Date(data.window_end) : null
  return (
    <div className="banner warn" role="status" style={{ marginBottom: 14 }}>
      <TriangleAlert size={16} style={{ flexShrink: 0 }} />
      <span className="grow">
        <strong>Maintenance mode is active.</strong>{' '}
        {data.note?.trim()
          ? data.note
          : 'Job dispatch is paused — running jobs finish; scheduled and queued work resumes when maintenance ends.'}
        {until && !Number.isNaN(until.getTime()) ? ` Until ${until.toLocaleString()}.` : ''}
      </span>
    </div>
  )
}
