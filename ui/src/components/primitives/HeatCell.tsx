export interface HeatCellProps {
  value: number
  max: number
  title?: string
}

export function HeatCell({ value, max, title }: HeatCellProps) {
  const v = max === 0 ? 0 : value / max
  let bg: string
  if (v === 0) bg = 'var(--bg-3)'
  else if (v < 0.2) bg = 'oklch(0.45 0.10 285 / 0.30)'
  else if (v < 0.4) bg = 'oklch(0.55 0.16 285 / 0.55)'
  else if (v < 0.6) bg = 'oklch(0.62 0.18 285 / 0.75)'
  else if (v < 0.8) bg = 'oklch(0.66 0.20 285 / 0.90)'
  else bg = 'oklch(0.70 0.22 285)'
  return (
    <div
      className="heat-cell"
      style={{ background: bg, border: '1px solid var(--border)' }}
      title={title ?? `${value} failures`}
    />
  )
}
