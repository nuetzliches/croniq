import { useEffect, useState } from 'react'
import { Trash2, Plus } from 'lucide-react'
import { formatCalendarRules, parseCalendarRules, type CalendarRulePayload } from '@/lib/croniq-dsl'
import { CopyButton } from '@/components/ui/copy-button'
import { TimezoneInput } from '@/components/ui/timezone-input'

const RULE_TYPES = ['weekly', 'window', 'monthly', 'annual', 'timezone'] as const

const RULE_TYPE_LABELS: Record<(typeof RULE_TYPES)[number], string> = {
  weekly: 'Weekdays',
  window: 'Time window',
  monthly: 'Days of month',
  annual: 'Specific date',
  timezone: 'Timezone',
}

// Per-rule-type one-line caption shown beside the rule editor. Less
// noisy than a tooltip, more discoverable than a placeholder, and
// stays put while the user fills in the structured controls.
const RULE_TYPE_HINTS: Record<(typeof RULE_TYPES)[number], string> = {
  weekly: 'Days when this rule applies.',
  window: 'Hour range (UTC inside the calendar; respect the calendar timezone).',
  monthly: 'Days of month — pick numbers or "Last".',
  annual: 'A single calendar date (no year — fires every year on this day).',
  timezone: 'IANA name. Type to search; the browser picker filters as you type.',
}

const WEEKDAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'] as const
const WEEKDAY_PRESETS: { label: string; days: (typeof WEEKDAYS)[number][] }[] = [
  { label: 'Weekday', days: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'] },
  { label: 'Weekend', days: ['Sat', 'Sun'] },
  { label: 'Every day', days: [...WEEKDAYS] },
]

const ORDINALS = [
  '1', '2', '3', '4', '5', '6', '7', '8', '9', '10',
  '11', '12', '13', '14', '15', '16', '17', '18', '19', '20',
  '21', '22', '23', '24', '25', '26', '27', '28', '29', '30',
  '31', 'last',
]
const MONTHLY_PRESETS: { label: string; days: string[] }[] = [
  { label: '1st', days: ['1'] },
  { label: '15th', days: ['15'] },
  { label: '1st + 15th', days: ['1', '15'] },
  { label: 'Last day', days: ['last'] },
]

const MONTHS = [
  'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
  'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec',
]

interface Props {
  /// Emitted whenever the form produces a valid DSL string. The dialog
  /// passes this through to the API on submit.
  onChange: (dsl: string) => void
  /// Surfaced parse errors — the dialog renders this inline so the user
  /// can fix the expression before submitting.
  onError?: (msg: string | null) => void
  /// Optional initial rule list. Defaults to the same Mon-Fri / no
  /// Christmas pair we ship in the standalone generator.
  initial?: CalendarRulePayload[]
}

const inputCls =
  'w-full px-3 py-2 border border-border rounded-md text-sm bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-primary'

const chipBase =
  'inline-flex items-center justify-center rounded-md text-xs font-mono transition-colors border'
const chipOff =
  'bg-background border-border text-muted-foreground hover:border-primary/50'
const chipOn =
  'bg-primary/15 border-primary text-primary'

export function CalendarRuleBuilder({ onChange, onError, initial }: Props) {
  const [rules, setRules] = useState<CalendarRulePayload[]>(
    initial ?? [
      { action: 'include', rule_type: 'weekly', args: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri'] },
      { action: 'exclude', rule_type: 'annual', args: ['12-25'] },
    ],
  )
  const [dsl, setDsl] = useState('')

  // Format → parse round-trip on every change. The format direction is
  // the source of truth fed to the API; the parse pass lets us catch
  // malformed args (e.g. an annual rule with text instead of MM-DD)
  // before submit and surface them inline.
  useEffect(() => {
    let cancelled = false
    ;(async () => {
      try {
        const formatted = await formatCalendarRules(rules)
        if (cancelled) return
        setDsl(formatted)
        onChange(formatted)
        const parsed = await parseCalendarRules(formatted)
        if (cancelled) return
        if (!parsed.ok) onError?.(parsed.diagnostics.join('\n'))
        else onError?.(null)
      } catch (e) {
        if (cancelled) return
        onError?.(e instanceof Error ? e.message : String(e))
      }
    })()
    return () => {
      cancelled = true
    }
  }, [rules, onChange, onError])

  function updateRule(idx: number, patch: Partial<CalendarRulePayload>) {
    setRules((prev) => prev.map((r, i) => (i === idx ? { ...r, ...patch } : r)))
  }

  function removeRule(idx: number) {
    setRules((prev) => prev.filter((_, i) => i !== idx))
  }

  function addRule() {
    setRules((prev) => [...prev, { action: 'include', rule_type: 'weekly', args: [] }])
  }

  return (
    <div className="space-y-3">
      {rules.map((rule, idx) => (
        <div key={idx} className="rounded-md border border-border bg-background/50 p-3 space-y-2">
          <div className="flex items-center gap-2">
            <select
              value={rule.action}
              onChange={(e) => updateRule(idx, { action: e.target.value as 'include' | 'exclude' })}
              className={`${inputCls} py-1.5 text-xs w-[88px]`}
              aria-label={`Rule ${idx + 1} action`}
            >
              <option value="include">include</option>
              <option value="exclude">exclude</option>
            </select>
            <select
              value={rule.rule_type}
              onChange={(e) => {
                // Reset args when the rule type changes — they have
                // type-specific shape and a stale carry-over would
                // silently mis-render the DSL.
                updateRule(idx, { rule_type: e.target.value, args: [] })
              }}
              className={`${inputCls} py-1.5 text-xs flex-1`}
              aria-label={`Rule ${idx + 1} type`}
            >
              {RULE_TYPES.map((t) => (
                <option key={t} value={t}>
                  {RULE_TYPE_LABELS[t]} ({t})
                </option>
              ))}
            </select>
            <button
              type="button"
              onClick={() => removeRule(idx)}
              aria-label={`Remove rule ${idx + 1}`}
              className="h-7 w-7 inline-flex items-center justify-center rounded-md text-muted-foreground hover:text-destructive"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          </div>
          <p className="text-[10px] text-muted-foreground leading-snug">
            {RULE_TYPE_HINTS[rule.rule_type as (typeof RULE_TYPES)[number]]}
          </p>
          <RuleEditor
            rule={rule}
            onChange={(args) => updateRule(idx, { args })}
          />
        </div>
      ))}
      <button
        type="button"
        onClick={addRule}
        className="w-full py-2 rounded-md border border-dashed border-border text-xs text-muted-foreground hover:text-primary hover:border-primary/50 inline-flex items-center justify-center gap-1.5"
      >
        <Plus className="h-3.5 w-3.5" />
        Add rule
      </button>
      <div>
        <div className="flex items-center justify-between mb-1">
          <label htmlFor="calendar-rule-builder-dsl" className="text-xs font-medium text-foreground">
            DSL
          </label>
          {dsl && <CopyButton value={dsl} label="Copy calendar DSL" />}
        </div>
        <output
          id="calendar-rule-builder-dsl"
          aria-live="polite"
          className="block text-xs bg-muted rounded-md p-2 font-mono whitespace-pre-wrap break-words"
        >
          {dsl || '(empty calendar — always on)'}
        </output>
      </div>
    </div>
  )
}

interface RuleEditorProps {
  rule: CalendarRulePayload
  onChange: (args: string[]) => void
}

/// Per-rule-type structured editor. Replaces the previous single-text
/// input — typing `mon..fri` worked but the calendar preview never
/// reacted, the args weren't validated until submit, and `weekly` /
/// `monthly` / `annual` shared one input shape that fit none of them
/// well. Each branch below produces the same `args: string[]` the
/// rest of the editor expects, so the WASM call site is unchanged.
function RuleEditor({ rule, onChange }: RuleEditorProps) {
  if (rule.rule_type === 'weekly') return <WeeklyEditor rule={rule} onChange={onChange} />
  if (rule.rule_type === 'window') return <WindowEditor rule={rule} onChange={onChange} />
  if (rule.rule_type === 'monthly') return <MonthlyEditor rule={rule} onChange={onChange} />
  if (rule.rule_type === 'annual') return <AnnualEditor rule={rule} onChange={onChange} />
  if (rule.rule_type === 'timezone') return <TimezoneEditor rule={rule} onChange={onChange} />
  return null
}

function WeeklyEditor({ rule, onChange }: RuleEditorProps) {
  // Stored args may be 3-letter (`Mon`) or full (`monday`); normalise
  // to 3-letter capitalised for both display and storage so the WASM
  // formatter sees a consistent shape it can collapse to `weekday` /
  // `Mon..Fri` / etc.
  const active = new Set(rule.args.map(normaliseDay).filter(Boolean) as string[])
  function toggle(day: string) {
    const next = new Set(active)
    if (next.has(day)) next.delete(day)
    else next.add(day)
    onChange([...WEEKDAYS].filter((d) => next.has(d)))
  }
  function applyPreset(days: readonly string[]) {
    onChange([...days])
  }
  return (
    <div className="space-y-2">
      <div className="grid grid-cols-7 gap-1">
        {WEEKDAYS.map((d) => {
          const on = active.has(d)
          return (
            <button
              key={d}
              type="button"
              onClick={() => toggle(d)}
              aria-pressed={on}
              className={`${chipBase} px-1 py-1.5 ${on ? chipOn : chipOff}`}
            >
              {d}
            </button>
          )
        })}
      </div>
      <div className="flex flex-wrap gap-1.5">
        {WEEKDAY_PRESETS.map((p) => (
          <button
            key={p.label}
            type="button"
            onClick={() => applyPreset(p.days)}
            className="text-[10px] uppercase tracking-wide text-muted-foreground hover:text-primary"
          >
            {p.label}
          </button>
        ))}
      </div>
    </div>
  )
}

function WindowEditor({ rule, onChange }: RuleEditorProps) {
  const [from, to] = [rule.args[0] ?? '', rule.args[1] ?? '']
  function set(idx: 0 | 1, value: string) {
    const next = [from, to] as [string, string]
    next[idx] = value
    // Drop trailing empty values so the formatter doesn't emit a half-
    // window like `"08:00".."` while the user is mid-edit.
    if (!next[0] && !next[1]) onChange([])
    else onChange(next.filter(Boolean))
  }
  return (
    <div className="flex items-center gap-2 text-xs">
      <input
        type="time"
        value={from}
        onChange={(e) => set(0, e.target.value)}
        aria-label="Window start (UTC)"
        className={`${inputCls} py-1.5 w-[100px]`}
      />
      <span className="text-muted-foreground">to</span>
      <input
        type="time"
        value={to}
        onChange={(e) => set(1, e.target.value)}
        aria-label="Window end (UTC)"
        className={`${inputCls} py-1.5 w-[100px]`}
      />
    </div>
  )
}

function MonthlyEditor({ rule, onChange }: RuleEditorProps) {
  const active = new Set(rule.args.map((a) => a.replace(/^(\d+)(st|nd|rd|th)$/i, '$1').toLowerCase()))
  function toggle(o: string) {
    const next = new Set(active)
    if (next.has(o)) next.delete(o)
    else next.add(o)
    // Sort numerics ascending and append `last` if present so the DSL
    // stays in a stable order between renders.
    const out = [...next]
      .sort((a, b) => {
        if (a === 'last') return 1
        if (b === 'last') return -1
        return parseInt(a, 10) - parseInt(b, 10)
      })
    onChange(out)
  }
  function applyPreset(days: string[]) {
    onChange([...days])
  }
  return (
    <div className="space-y-2">
      <div className="grid grid-cols-8 gap-1">
        {ORDINALS.map((o) => {
          const on = active.has(o.toLowerCase())
          return (
            <button
              key={o}
              type="button"
              onClick={() => toggle(o)}
              aria-pressed={on}
              className={`${chipBase} px-1 py-1 ${on ? chipOn : chipOff}`}
            >
              {o}
            </button>
          )
        })}
      </div>
      <div className="flex flex-wrap gap-1.5">
        {MONTHLY_PRESETS.map((p) => (
          <button
            key={p.label}
            type="button"
            onClick={() => applyPreset(p.days)}
            className="text-[10px] uppercase tracking-wide text-muted-foreground hover:text-primary"
          >
            {p.label}
          </button>
        ))}
      </div>
    </div>
  )
}

function AnnualEditor({ rule, onChange }: RuleEditorProps) {
  // Stored as a single MM-DD string. Split into separate month/day
  // controls so the user gets a labelled month dropdown + a numeric
  // day input instead of a free-form `12-25` text field. Empty rule
  // = empty args; we only emit the formatted string once both halves
  // are filled.
  const current = rule.args[0] ?? ''
  const m = /^(\d{1,2})-(\d{1,2})$/.exec(current)
  const month = m ? parseInt(m[1], 10) : 0
  const day = m ? parseInt(m[2], 10) : 0

  function emit(nextMonth: number, nextDay: number) {
    if (!nextMonth || !nextDay) {
      onChange([])
      return
    }
    const mm = String(nextMonth).padStart(2, '0')
    const dd = String(nextDay).padStart(2, '0')
    onChange([`${mm}-${dd}`])
  }
  return (
    <div className="flex items-center gap-2 text-xs">
      <select
        value={month}
        onChange={(e) => emit(parseInt(e.target.value, 10), day)}
        aria-label="Month"
        className={`${inputCls} py-1.5 text-xs w-[100px]`}
      >
        <option value={0}>Month…</option>
        {MONTHS.map((label, i) => (
          <option key={label} value={i + 1}>
            {label}
          </option>
        ))}
      </select>
      <input
        type="number"
        min={1}
        max={31}
        value={day || ''}
        placeholder="Day"
        onChange={(e) => emit(month, parseInt(e.target.value, 10) || 0)}
        aria-label="Day of month"
        className={`${inputCls} py-1.5 text-xs w-[80px]`}
      />
      {month > 0 && day > 0 && (
        <span className="text-muted-foreground tabular-nums">
          {MONTHS[month - 1]} {day}
        </span>
      )}
    </div>
  )
}

function TimezoneEditor({ rule, onChange }: RuleEditorProps) {
  // Same TimezoneInput the Schedule builder uses — typeahead over the
  // browser's full IANA list, with the detected default surfaced as a
  // hint while the input is empty.
  const value = rule.args[0] ?? ''
  return (
    <TimezoneInput
      defaultValue={value}
      onChange={(e) => {
        const v = e.currentTarget.value.trim()
        onChange(v ? [v] : [])
      }}
      onBlur={(e) => {
        const v = e.currentTarget.value.trim()
        onChange(v ? [v] : [])
      }}
      showDetectedHint={!value}
      className={`${inputCls} py-1.5 text-xs`}
    />
  )
}

/// Normalise a weekday token to its capitalised 3-letter form
/// (`Mon`, `Tue`, ..., `Sun`). Returns `null` if the input doesn't
/// look like a weekday — used to filter out garbage when the user
/// types into the Advanced (raw) tab and switches back.
function normaliseDay(s: string): string | null {
  const lower = s.toLowerCase().slice(0, 3)
  switch (lower) {
    case 'mon': return 'Mon'
    case 'tue': return 'Tue'
    case 'wed': return 'Wed'
    case 'thu': return 'Thu'
    case 'fri': return 'Fri'
    case 'sat': return 'Sat'
    case 'sun': return 'Sun'
    default: return null
  }
}
