import clsx from 'clsx'

export type RunOutcome = 'ok' | 'warn' | 'err' | 'skip'

export interface RunBarsProps {
  counts: RunOutcome[]
  compact?: boolean
  className?: string
}

export function RunBars({ counts, compact = false, className }: RunBarsProps) {
  return (
    <span className={clsx('run-bars', className)} aria-hidden>
      {counts.map((c, i) => {
        const h = compact ? 6 + (i % 6) * 1.2 : 8 + ((i * 7) % 6) * 1.4
        return <span key={i} className={`bar ${c}`} style={{ height: `${h}px` }} />
      })}
    </span>
  )
}
