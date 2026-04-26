import {
  forwardRef,
  useCallback,
  useEffect,
  useId,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'

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
    // The listbox lives in a body-level portal so the outside-click
    // listener has to know about it explicitly — without this ref a
    // click on the listbox would land outside `containerRef`, the
    // popup would close, and the li's `onMouseDown` would never get to
    // fire its `commit()`.
    const listboxRef = useRef<HTMLUListElement | null>(null)
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
    // The listbox renders into a portal at <body> level so it isn't
    // clipped by the dialog's `overflow-y-auto`. We compute its
    // viewport-fixed coordinates from the input's bounding rect.
    const [popPos, setPopPos] = useState<{ top: number; left: number; width: number } | null>(null)

    const recomputePos = useCallback(() => {
      const el = inputRef.current
      if (!el) return
      const r = el.getBoundingClientRect()
      // Anchor 4 px below the input bottom — same offset we had with
      // the in-flow absolute version, just in viewport coords now.
      setPopPos({ top: r.bottom + 4, left: r.left, width: r.width })
    }, [])

    // Recompute on open and on every scroll/resize while open. The
    // listbox sits in a portal at <body> so any container that scrolls
    // (the dialog's content area, the page itself) needs to nudge the
    // popup back under the input.
    useLayoutEffect(() => {
      if (!open) return
      recomputePos()
      const onScroll = () => recomputePos()
      window.addEventListener('scroll', onScroll, true) // capture: catches nested scrollers
      window.addEventListener('resize', onScroll)
      return () => {
        window.removeEventListener('scroll', onScroll, true)
        window.removeEventListener('resize', onScroll)
      }
    }, [open, recomputePos])

    // Filter on every keystroke. ~430 entries × case-insensitive
    // includes() is sub-millisecond — no need to memoise the filter.
    const filtered = useMemo(() => {
      const q = value.trim().toLowerCase()
      if (!q) return zones.slice(0, 200)
      return zones.filter((tz) => tz.toLowerCase().includes(q)).slice(0, 200)
    }, [value, zones])

    // Close on outside click. The listbox is portaled to body, so the
    // input's container alone isn't enough — without `listboxRef` the
    // popup would close before the option's `onMouseDown` could call
    // `commit()`, and the click would appear to do nothing.
    useEffect(() => {
      function onDocMouseDown(e: MouseEvent) {
        const target = e.target as Node
        if (containerRef.current?.contains(target)) return
        if (listboxRef.current?.contains(target)) return
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
        // Re-focus the input. Some click pipelines (CDP, certain
        // a11y tools, pen taps) shift focus to <body> before our
        // handler runs, even with `mousedown.preventDefault` — pulling
        // focus back here makes selection robust across all paths.
        el.focus()
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

    // A non-empty value that doesn't match any IANA name flags the
    // field as invalid — same UX as a "this isn't a real timezone"
    // hint. Empty stays valid (user wants UTC fallback). Free text
    // matching a known zone is accepted; only typos fail.
    //
    // We surface this two ways:
    //   1. CSS — red border via `aria-invalid` + a tailwind variant
    //   2. ARIA — assistive tech hears "invalid"
    const isInvalid = value !== '' && zones.length > 0 && !zones.includes(value)

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
          aria-invalid={isInvalid || undefined}
          role="combobox"
          className={`${className ?? ''} ${
            // The invalid state nudges the border red without depending
            // on RHF — makes the typo obvious before the user even
            // tries to submit.
            isInvalid ? '!border-destructive focus:!ring-destructive' : ''
          }`}
          {...rest}
        />
        {/* Listbox renders into a body-level portal so the dialog's
            `overflow-y-auto` doesn't clip it. Position is viewport-
            fixed and recomputed on scroll/resize. `z-[60]` sits above
            the Radix dialog overlay (`z-50`) and content (`z-50`) so
            the popup is interactive even while the dialog is open. */}
        {open && filtered.length > 0 && popPos &&
          createPortal(
            <ul
              ref={listboxRef}
              id={listId}
              role="listbox"
              style={{
                position: 'fixed',
                top: popPos.top,
                left: popPos.left,
                width: popPos.width,
              }}
              className="z-[60] max-h-60 overflow-y-auto rounded-md border border-border bg-card shadow-lg"
            >
              {filtered.map((tz, idx) => (
                <li
                  key={tz}
                  role="option"
                  aria-selected={idx === activeIdx}
                  // Combobox pattern: `mousedown.preventDefault` keeps
                  // input focused (no blur). Commit on `click` so the
                  // listbox stays mounted across the full click cycle.
                  // `commit()` also re-focuses the input as a fallback
                  // for pipelines where `preventDefault` was bypassed.
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => commit(tz)}
                  onMouseEnter={() => setActiveIdx(idx)}
                  className={`px-3 py-1.5 text-xs font-mono cursor-pointer ${
                    idx === activeIdx ? 'bg-accent text-accent-foreground' : 'text-foreground'
                  }`}
                >
                  {tz}
                </li>
              ))}
            </ul>,
            document.body,
          )}
        {isInvalid && (
          <p className="text-[11px] text-destructive mt-1">
            Not a known IANA timezone — pick one from the list or check the spelling.
          </p>
        )}
        {!isInvalid && showDetectedHint && value && value === browserTz && (
          <p className="text-[11px] text-muted-foreground mt-1">
            Detected from your browser — type to search or paste any IANA name.
          </p>
        )}
      </div>
    )
  },
)
