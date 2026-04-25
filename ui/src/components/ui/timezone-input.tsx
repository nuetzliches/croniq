import { forwardRef, useEffect, useId, useImperativeHandle, useMemo, useRef, useState } from 'react'

interface TimezoneInputProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'onChange'> {
  /// React Hook Form passes its `onChange` through `register`. We
  /// preserve the standard signature so the field can be `register`-ed
  /// directly without an adapter.
  onChange?: React.ChangeEventHandler<HTMLInputElement>
  /// Show the small "Detected from browser" hint while the value still
  /// matches the browser-detected default.
  showDetectedHint?: boolean
}

/// IANA timezone picker. Backed by `Intl.supportedValuesOf('timeZone')`
/// for the suggestion list and `Intl.DateTimeFormat().resolvedOptions().timeZone`
/// for the default. Custom dropdown (not `<datalist>`) so we control
/// height + anchor — native datalists are UA-positioned and the popup
/// flickers as the result count changes.
///
/// Behaviour:
///   - Free text always allowed; the dropdown is suggestion-only.
///   - Substring match (case-insensitive) — typing `vienna` narrows
///     to `Europe/Vienna`.
///   - Up/Down arrow keys navigate, Enter selects the highlighted
///     option, Esc closes.
///   - Click outside closes.
export const TimezoneInput = forwardRef<HTMLInputElement, TimezoneInputProps>(
  function TimezoneInput(
    { defaultValue, showDetectedHint, onChange, onBlur, name, className, ...rest },
    ref,
  ) {
    const listId = useId()
    const inputRef = useRef<HTMLInputElement | null>(null)
    // Stitch the forwarded ref together with our internal one — RHF
    // wants the DOM node, our keyboard handler wants the same node.
    useImperativeHandle(ref, () => inputRef.current as HTMLInputElement, [])

    const containerRef = useRef<HTMLDivElement | null>(null)
    // Flag to suppress the dropdown re-opening from the synthesized
    // `input` event we fire inside `commit()`. Without this, picking an
    // option keeps the listbox open because our own onChange handler
    // calls `setOpen(true)` on every input event.
    const committingRef = useRef(false)

    const { zones, browserTz } = useMemo(() => {
      let zones: string[] = []
      try {
        if (typeof Intl.supportedValuesOf === 'function') {
          zones = Intl.supportedValuesOf('timeZone')
        }
      } catch {
        // Ancient browser — empty list, free text still works.
      }
      let browserTz = ''
      try {
        browserTz = Intl.DateTimeFormat().resolvedOptions().timeZone || ''
      } catch {
        // Sandbox without Intl.DateTimeFormat — leave empty.
      }
      return { zones, browserTz }
    }, [])

    const initial = (defaultValue ?? browserTz) as string
    const [value, setValue] = useState(initial)
    const [open, setOpen] = useState(false)
    const [activeIdx, setActiveIdx] = useState(0)

    // Filter on every keystroke. ~430 entries × case-insensitive
    // includes() is sub-millisecond — no need to memoise the filter.
    const filtered = useMemo(() => {
      const q = value.trim().toLowerCase()
      if (!q) return zones.slice(0, 200)
      return zones.filter((tz) => tz.toLowerCase().includes(q)).slice(0, 200)
    }, [value, zones])

    // Close on outside click. Use mousedown so the click on a list
    // option (handled below) still fires before the close runs.
    useEffect(() => {
      function onDocMouseDown(e: MouseEvent) {
        if (!containerRef.current) return
        if (containerRef.current.contains(e.target as Node)) return
        setOpen(false)
      }
      if (open) document.addEventListener('mousedown', onDocMouseDown)
      return () => document.removeEventListener('mousedown', onDocMouseDown)
    }, [open])

    function commit(next: string) {
      committingRef.current = true
      setValue(next)
      setOpen(false)
      // Synthesize a change event so RHF picks up the new value.
      const el = inputRef.current
      if (el) {
        const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')!.set!
        setter.call(el, next)
        el.dispatchEvent(new Event('input', { bubbles: true }))
      }
      // Drop the flag on the next macrotask — by then the synthesized
      // input event has already been processed by the onChange handler.
      setTimeout(() => {
        committingRef.current = false
      }, 0)
    }

    function handleKey(e: React.KeyboardEvent<HTMLInputElement>) {
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        if (!open) setOpen(true)
        setActiveIdx((i) => Math.min(filtered.length - 1, i + 1))
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        setActiveIdx((i) => Math.max(0, i - 1))
      } else if (e.key === 'Enter') {
        if (open && filtered[activeIdx]) {
          e.preventDefault()
          commit(filtered[activeIdx])
        }
      } else if (e.key === 'Escape') {
        setOpen(false)
      }
    }

    return (
      <div ref={containerRef} className="relative">
        <input
          ref={inputRef}
          name={name}
          value={value}
          onChange={(e) => {
            setValue(e.target.value)
            setActiveIdx(0)
            // Don't re-open the dropdown when our own `commit()` fired
            // the synthesized input event — the user just selected an
            // option and the popup should stay closed.
            if (!committingRef.current) setOpen(true)
            onChange?.(e)
          }}
          onFocus={() => setOpen(true)}
          onBlur={onBlur}
          onKeyDown={handleKey}
          placeholder="Timezone (e.g. Europe/Vienna)"
          autoComplete="off"
          aria-autocomplete="list"
          aria-controls={listId}
          aria-expanded={open}
          role="combobox"
          className={className}
          {...rest}
        />
        {open && filtered.length > 0 && (
          <ul
            id={listId}
            role="listbox"
            // `max-h-60` ≈ 240 px; the input height + this fits inside
            // the dialog without overlapping the action bar at the
            // bottom. `overflow-y-auto` keeps overflow inside the
            // dropdown rather than letting the document scroll.
            className="absolute left-0 right-0 z-50 mt-1 max-h-60 overflow-y-auto rounded-md border border-border bg-card shadow-lg"
          >
            {filtered.map((tz, idx) => (
              <li
                key={tz}
                role="option"
                aria-selected={idx === activeIdx}
                onMouseDown={(e) => {
                  // Prevent the input's blur firing before our select.
                  e.preventDefault()
                  commit(tz)
                }}
                onMouseEnter={() => setActiveIdx(idx)}
                className={`px-3 py-1.5 text-xs font-mono cursor-pointer ${
                  idx === activeIdx ? 'bg-accent text-accent-foreground' : 'text-foreground'
                }`}
              >
                {tz}
              </li>
            ))}
          </ul>
        )}
        {showDetectedHint && value && value === browserTz && (
          <p className="text-[11px] text-muted-foreground mt-1">
            Detected from your browser — type to search or paste any IANA name.
          </p>
        )}
      </div>
    )
  },
)
