import { forwardRef, useId, useMemo } from 'react'

interface TimezoneInputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  /// Show the small "Detected from browser" hint below the field while
  /// the value still equals the browser-detected default. Suppressed
  /// once the user types something else.
  showDetectedHint?: boolean
}

/// IANA timezone picker built on `<datalist>` so the browser can do
/// substring matching natively (typing `vienna` narrows to
/// `Europe/Vienna` on Chromium and Firefox; Safari matches by prefix
/// only — both behaviours are acceptable since IANA names are
/// continent-prefixed by convention).
///
/// Uses two ECMAScript-2022 Intl APIs:
///   - `Intl.supportedValuesOf('timeZone')` — runtime tz list, ~430
///     entries on a 2026 Chromium. Wrapped in try/catch so older
///     browsers degrade to a plain free-text input.
///   - `Intl.DateTimeFormat().resolvedOptions().timeZone` — the user's
///     browser timezone, used as `defaultValue` if the caller didn't
///     supply one.
export const TimezoneInput = forwardRef<HTMLInputElement, TimezoneInputProps>(
  function TimezoneInput({ defaultValue, showDetectedHint, ...rest }, ref) {
    const listId = useId()
    // Memoise — the IANA list is stable for the page lifetime, no point
    // re-querying or rebuilding the option DOM on every render.
    const { zones, browserTz } = useMemo(() => {
      let zones: string[] = []
      try {
        // `Intl.supportedValuesOf` is ES2022. Check at call-site so a
        // missing implementation doesn't throw a hard error before the
        // try/catch can swallow it.
        if (typeof Intl.supportedValuesOf === 'function') {
          zones = Intl.supportedValuesOf('timeZone')
        }
      } catch {
        // Old browser — fall through to empty list, the input still
        // accepts free text.
      }
      let browserTz = ''
      try {
        browserTz = Intl.DateTimeFormat().resolvedOptions().timeZone || ''
      } catch {
        // Some sandboxed environments won't expose this either; the
        // input then renders without a default, which is fine.
      }
      return { zones, browserTz }
    }, [])

    const initial = defaultValue ?? browserTz
    return (
      <div className="space-y-1">
        <input
          ref={ref}
          list={listId}
          defaultValue={initial}
          placeholder="Timezone (e.g. Europe/Vienna)"
          {...rest}
        />
        <datalist id={listId}>
          {zones.map((tz) => (
            <option key={tz} value={tz} />
          ))}
        </datalist>
        {showDetectedHint && initial && initial === browserTz && (
          <p className="text-[11px] text-muted-foreground">
            Detected from your browser — type to search or paste any IANA name.
          </p>
        )}
      </div>
    )
  },
)
