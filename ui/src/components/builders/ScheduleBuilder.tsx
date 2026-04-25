import { useEffect, useState } from 'react'
import { formatSchedule, nextFires, type SchedulePayload } from '@/lib/croniq-dsl'

const WEEKDAYS = ['monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday', 'sunday']
const WEEKDAY_SHORT: Record<string, string> = {
  monday: 'Mon', tuesday: 'Tue', wednesday: 'Wed', thursday: 'Thu',
  friday: 'Fri', saturday: 'Sat', sunday: 'Sun',
}
// Ordinals 1st..31st + last. The standalone generator uses the same
// list; keeping them in sync via a shared array would mean either
// duplicating it across `site/` (no module system) or pulling it from
// here (then `site/generator.js` becomes a build artefact). Two
// strings × 32 entries is cheap enough to live in both places.
const ORDINALS = [
  '1st', '2nd', '3rd', '4th', '5th', '6th', '7th', '8th', '9th', '10th',
  '11th', '12th', '13th', '14th', '15th', '16th', '17th', '18th', '19th', '20th',
  '21st', '22nd', '23rd', '24th', '25th', '26th', '27th', '28th', '29th', '30th',
  '31st', 'last',
]

interface Props {
  /// Emitted whenever the form produces a valid DSL string. The dialog
  /// passes this value through to the API on submit.
  onChange: (dsl: string) => void
  /// Optional initial state. Defaults to `every 5 minutes`.
  initial?: SchedulePayload
}

const inputCls =
  'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

export function ScheduleBuilder({ onChange, initial }: Props) {
  const [payload, setPayload] = useState<SchedulePayload>(
    initial ?? { mode: 'interval', count: 5, unit: 'minutes' },
  )
  const [dsl, setDsl] = useState('')
  const [fires, setFires] = useState<string[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  // Refresh DSL + next-fires preview every time the payload changes.
  // Both calls go through the wasm bridge; the lazy init in
  // `lib/croniq-dsl.ts` makes only the first one wait for the
  // download.
  useEffect(() => {
    let cancelled = false
    ;(async () => {
      try {
        const formatted = await formatSchedule(payload)
        if (cancelled) return
        setDsl(formatted)
        onChange(formatted)
        setError(null)
        const now = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z')
        const result = await nextFires(formatted, now, 5)
        if (cancelled) return
        setFires(result.ok ? result.fires : null)
      } catch (e) {
        if (cancelled) return
        setError(e instanceof Error ? e.message : String(e))
      }
    })()
    return () => {
      cancelled = true
    }
  }, [payload, onChange])

  return (
    <div className="space-y-3">
      <div>
        <label className="text-xs font-medium text-foreground block mb-1">Mode</label>
        <select
          value={payload.mode}
          onChange={(e) => {
            const m = e.target.value
            // Re-init the payload with the defaults for the chosen mode.
            // Carrying over fields between modes would surface stale
            // values (e.g. switching daily→weekdays would inherit the
            // hour but lose any meaningful day list).
            if (m === 'interval') setPayload({ mode: 'interval', count: 5, unit: 'minutes' })
            else if (m === 'daily') setPayload({ mode: 'daily', hour: 9, minute: 0 })
            else if (m === 'weekdays') setPayload({
              mode: 'weekdays',
              days: ['monday', 'tuesday', 'wednesday', 'thursday', 'friday'],
              hour: 9, minute: 0,
            })
            else if (m === 'monthly') setPayload({
              mode: 'monthly', ordinals: ['1st'], hour: 3, minute: 0,
            })
            else if (m === 'once') setPayload({ mode: 'once', at: '2026-12-31T23:00:00Z' })
            else setPayload({ mode: 'disabled' })
          }}
          className={inputCls}
        >
          <option value="interval">Interval (every N seconds/minutes/hours)</option>
          <option value="daily">Daily (every day at HH:MM)</option>
          <option value="weekdays">Weekdays (specific days at HH:MM)</option>
          <option value="monthly">Monthly (specific days of month)</option>
          <option value="once">Once (at a specific UTC time)</option>
          <option value="disabled">Disabled (won't fire)</option>
        </select>
      </div>

      {payload.mode === 'interval' && (
        <div className="grid grid-cols-2 gap-2">
          <div>
            <label className="text-xs font-medium text-foreground block mb-1">Count</label>
            <input
              type="number"
              min={1}
              value={payload.count}
              onChange={(e) =>
                setPayload({ ...payload, count: Math.max(1, parseInt(e.target.value, 10) || 1) })
              }
              className={inputCls}
            />
          </div>
          <div>
            <label className="text-xs font-medium text-foreground block mb-1">Unit</label>
            <select
              value={payload.unit}
              onChange={(e) =>
                setPayload({ ...payload, unit: e.target.value as 'seconds' | 'minutes' | 'hours' })
              }
              className={inputCls}
            >
              <option value="seconds">seconds</option>
              <option value="minutes">minutes</option>
              <option value="hours">hours</option>
            </select>
          </div>
        </div>
      )}

      {(payload.mode === 'daily' || payload.mode === 'weekdays' || payload.mode === 'monthly') && (
        <div>
          <label className="text-xs font-medium text-foreground block mb-1">Time (UTC)</label>
          <input
            type="time"
            value={`${String(payload.hour).padStart(2, '0')}:${String(payload.minute).padStart(2, '0')}`}
            onChange={(e) => {
              const [h, m] = (e.target.value || '0:0').split(':').map((s) => parseInt(s, 10) || 0)
              setPayload({ ...payload, hour: h, minute: m })
            }}
            className={inputCls}
          />
        </div>
      )}

      {payload.mode === 'weekdays' && (
        <div>
          <label className="text-xs font-medium text-foreground block mb-1">Days</label>
          <div className="grid grid-cols-7 gap-1">
            {WEEKDAYS.map((d) => {
              const active = payload.days.includes(d)
              return (
                <button
                  key={d}
                  type="button"
                  onClick={() => {
                    const next = active
                      ? payload.days.filter((x) => x !== d)
                      : [...payload.days, d]
                    setPayload({ ...payload, days: next })
                  }}
                  className={`px-1 py-1.5 rounded-md text-xs font-mono transition-colors ${
                    active
                      ? 'bg-primary/15 border border-primary text-primary'
                      : 'bg-background border border-border text-muted-foreground hover:border-primary/50'
                  }`}
                >
                  {WEEKDAY_SHORT[d]}
                </button>
              )
            })}
          </div>
        </div>
      )}

      {payload.mode === 'monthly' && (
        <div>
          <label className="text-xs font-medium text-foreground block mb-1">Day(s) of month</label>
          <div className="grid grid-cols-8 gap-1">
            {ORDINALS.map((o) => {
              const active = payload.ordinals.includes(o)
              return (
                <button
                  key={o}
                  type="button"
                  onClick={() => {
                    const next = active
                      ? payload.ordinals.filter((x) => x !== o)
                      : [...payload.ordinals, o]
                    setPayload({ ...payload, ordinals: next })
                  }}
                  className={`px-1 py-1.5 rounded-md text-xs font-mono transition-colors ${
                    active
                      ? 'bg-primary/15 border border-primary text-primary'
                      : 'bg-background border border-border text-muted-foreground hover:border-primary/50'
                  }`}
                >
                  {o}
                </button>
              )
            })}
          </div>
        </div>
      )}

      {payload.mode === 'once' && (
        <div>
          <label className="text-xs font-medium text-foreground block mb-1">UTC datetime</label>
          <input
            type="text"
            value={payload.at}
            onChange={(e) => setPayload({ ...payload, at: e.target.value })}
            placeholder="2026-12-31T23:00:00Z"
            className={inputCls}
          />
        </div>
      )}

      {payload.mode === 'disabled' && (
        <p className="text-xs text-muted-foreground">
          Disabled jobs stay in the Croniqfile but never fire — useful during
          incident triage when you want the config in source control without
          triggers enqueueing.
        </p>
      )}

      <div>
        <label className="text-xs font-medium text-foreground block mb-1">DSL</label>
        <pre className="text-xs bg-muted rounded-md p-2 font-mono break-words whitespace-pre-wrap">
          {dsl || '(loading…)'}
        </pre>
      </div>

      {error && <p className="text-xs text-destructive">{error}</p>}

      {fires && fires.length > 0 && payload.mode !== 'disabled' && (
        <div>
          <label className="text-xs font-medium text-foreground block mb-1">Next 5 fires (UTC)</label>
          <ul className="text-xs font-mono text-muted-foreground space-y-0.5">
            {fires.map((f) => (
              <li key={f}>{f}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  )
}
