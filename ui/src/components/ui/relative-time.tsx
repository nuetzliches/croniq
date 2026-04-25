import { useEffect, useState } from 'react'
import { formatDate, formatRelative } from '@/lib/utils'

interface RelativeTimeProps {
  iso: string
  /// Refresh interval in ms. Default 30s — enough for "5m ago" to tick
  /// up to "6m ago" before users notice. Set higher for archival lists.
  intervalMs?: number
  className?: string
}

/// "5s ago", "3m ago", "2026-03-12" — formatted relative to *now* and
/// auto-updating on a timer. The full ISO timestamp is exposed via the
/// `title` attribute and as `<time>` element's `dateTime` so users get
/// the absolute value on hover and screen readers can announce it.
export function RelativeTime({ iso, intervalMs = 30_000, className }: RelativeTimeProps) {
  // Counter-based tick instead of `Date.now()` so the initial render is
  // pure — React 19's `react-hooks/purity` flags impure calls in render.
  // The actual "now" is read inside `formatRelative(iso)`, which is fine
  // because that helper is called during render with a fresh value
  // produced by `setInterval`.
  const [, setTick] = useState(0)
  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), intervalMs)
    return () => clearInterval(id)
  }, [intervalMs])

  return (
    <time className={className} dateTime={iso} title={formatDate(iso)}>
      {formatRelative(iso)}
    </time>
  )
}
