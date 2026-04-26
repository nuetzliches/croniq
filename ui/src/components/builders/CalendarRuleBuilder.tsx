import { useEffect, useState } from 'react'
import { Trash2, Plus } from 'lucide-react'
import { formatCalendarRules, parseCalendarRules, type CalendarRulePayload } from '@/lib/croniq-dsl'
import { CopyButton } from '@/components/ui/copy-button'

const RULE_TYPES = ['weekly', 'window', 'monthly', 'annual', 'timezone'] as const
const RULE_ARG_HINTS: Record<string, string> = {
  weekly: 'Days: e.g. Mon Tue Wed (3-letter, space-separated)',
  window: 'Window: e.g. 08:00..18:00',
  monthly: 'Days: e.g. 1 15 ("last" allowed)',
  annual: 'Date: MM-DD (e.g. 12-25)',
  timezone: 'IANA name: e.g. Europe/Vienna',
}

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

  function argsAsText(rule: CalendarRulePayload): string {
    // `window` is `"HH:MM".."HH:MM"` — round-trip through `..` so the
    // text input feels natural. Other rule types are space-separated
    // tokens.
    if (rule.rule_type === 'window' && rule.args.length === 2) {
      return `${rule.args[0]}..${rule.args[1]}`
    }
    return rule.args.join(' ')
  }

  function textToArgs(ruleType: string, text: string): string[] {
    const t = text.trim()
    if (!t) return []
    if (ruleType === 'window') {
      const [a, b] = t.split('..').map((s) => s.replace(/^"|"$/g, '').trim())
      return a && b ? [a, b] : [t]
    }
    return t.split(/\s+/)
  }

  return (
    <div className="space-y-2">
      {rules.map((rule, idx) => (
        <div key={idx} className="grid grid-cols-[88px_92px_1fr_auto] gap-2 items-center">
          <select
            value={rule.action}
            onChange={(e) => updateRule(idx, { action: e.target.value as 'include' | 'exclude' })}
            className={`${inputCls} py-1.5 text-xs`}
          >
            <option value="include">include</option>
            <option value="exclude">exclude</option>
          </select>
          <select
            value={rule.rule_type}
            onChange={(e) => {
              // Reset args when the rule type changes — they have type-
              // specific shape and a stale carry-over would silently
              // mis-render the DSL.
              updateRule(idx, { rule_type: e.target.value, args: [] })
            }}
            className={`${inputCls} py-1.5 text-xs`}
          >
            {RULE_TYPES.map((t) => (
              <option key={t} value={t}>{t}</option>
            ))}
          </select>
          <input
            type="text"
            value={argsAsText(rule)}
            onChange={(e) => updateRule(idx, { args: textToArgs(rule.rule_type, e.target.value) })}
            placeholder={RULE_ARG_HINTS[rule.rule_type] || ''}
            className={`${inputCls} py-1.5 text-xs`}
          />
          <button
            type="button"
            onClick={() => removeRule(idx)}
            aria-label={`Remove rule ${idx + 1}`}
            className="h-7 w-7 inline-flex items-center justify-center rounded-md text-muted-foreground hover:text-destructive"
          >
            <Trash2 className="h-3.5 w-3.5" />
          </button>
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
          {/* Copy is useful when the user wants to paste the rules into a
              hand-edited Croniqfile rather than (or in addition to)
              saving them as a Calendar resource. Hide while empty so a
              click never copies the placeholder hint. */}
          {dsl && <CopyButton value={dsl} label="Copy calendar DSL" />}
        </div>
        {/*
          `<output>` semantically marks this as the form's computed
          result, and `aria-live="polite"` lets screen readers announce
          updates as the user toggles rules. The previous `<pre>` was
          invisible to assistive tech.
        */}
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
