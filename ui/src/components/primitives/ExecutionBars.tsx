import clsx from 'clsx'

export type ExecutionOutcome = 'ok' | 'warn' | 'err' | 'skip'

export interface ExecutionBarsProps {
  counts: ExecutionOutcome[]
  /** Optional per-bar duration in ms — bar heights scale with duration. */
  durations?: (number | null | undefined)[]
  compact?: boolean
  className?: string
}

export function ExecutionBars({ counts, durations, compact = false, className }: ExecutionBarsProps) {
  const minH = compact ? 4 : 6
  const maxH = compact ? 14 : 20
  const flatH = compact ? 10 : 14

  // Real durations → log-scale to flatten huge spread between fast/slow executions.
  const valid = (durations ?? []).filter((d): d is number => typeof d === 'number' && d > 0)
  const hasDurations = valid.length > 0
  const lo = hasDurations ? Math.log10(Math.min(...valid)) : 0
  const hi = hasDurations ? Math.log10(Math.max(...valid)) : 0
  const span = Math.max(hi - lo, 0.001)

  return (
    <span className={clsx('execution-bars', className)} aria-hidden>
      {counts.map((c, i) => {
        const d = durations?.[i]
        let h = flatH
        if (hasDurations) {
          h = typeof d === 'number' && d > 0
            ? minH + ((Math.log10(d) - lo) / span) * (maxH - minH)
            : minH
        }
        return <span key={i} className={`bar ${c}`} style={{ height: `${h}px` }} />
      })}
    </span>
  )
}
